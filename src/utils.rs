use std::str::FromStr;

use anyhow::Context;
use axelar_wasm_std::msg_id::HexTxHash;
use sentry::ClientInitGuard;
use sentry_tracing::{layer as sentry_layer, EventFilter};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::{level_filters::LevelFilter, warn, Level};
use tracing_subscriber::{fmt, prelude::*, Registry};
use xrpl_amplifier_types::{
    msg::XRPLMessage,
    types::{XRPLPaymentAmount, XRPLToken, XRPLTokenAmount},
};
use xrpl_api::{Amount, Memo, PaymentTransaction, Transaction, TxRequest};

use crate::{
    config::Config,
    error::{GmpApiError, IngestorError},
    gmp_api::gmp_types::{
        CommonTaskFields, ConstructProofTask, ExecuteTask, GatewayTxTask, ReactToWasmEventTask,
        RefundTask, Task, TaskMetadata, VerifyTask, WasmEvent,
    },
};

fn parse_as<T: DeserializeOwned>(value: &Value) -> Result<T, GmpApiError> {
    serde_json::from_value(value.clone()).map_err(|e| GmpApiError::InvalidResponse(e.to_string()))
}

pub fn parse_task(task_json: &Value) -> Result<Task, GmpApiError> {
    let task_headers: CommonTaskFields = serde_json::from_value(task_json.clone())
        .map_err(|e| GmpApiError::InvalidResponse(e.to_string()))?;

    match task_headers.r#type.as_str() {
        "CONSTRUCT_PROOF" => {
            let task: ConstructProofTask = parse_as(task_json)?;
            Ok(Task::ConstructProof(task))
        }
        "GATEWAY_TX" => {
            let task: GatewayTxTask = parse_as(task_json)?;
            Ok(Task::GatewayTx(task))
        }
        "VERIFY" => {
            let task: VerifyTask = parse_as(task_json)?;
            Ok(Task::Verify(task))
        }
        "EXECUTE" => {
            let task: ExecuteTask = parse_as(task_json)?;
            Ok(Task::Execute(task))
        }
        "REFUND" => {
            let task: RefundTask = parse_as(task_json)?;
            Ok(Task::Refund(task))
        }
        "REACT_TO_WASM_EVENT" => {
            let task: ReactToWasmEventTask = parse_as(task_json)?;
            Ok(Task::ReactToWasmEvent(task))
        }
        _ => {
            let error_message = format!("Failed to parse task: {:?}", task_json);
            warn!(error_message);
            Err(GmpApiError::InvalidResponse(error_message))
        }
    }
}

pub fn extract_from_xrpl_memo(
    memos: Option<Vec<Memo>>,
    memo_type: &str,
) -> Result<String, anyhow::Error> {
    let memos = memos.ok_or_else(|| anyhow::anyhow!("No memos"))?;
    let desired_type_hex = hex::encode(memo_type).to_lowercase();

    if let Some(memo) = memos.into_iter().find(|m| {
        m.memo_type
            .as_ref()
            .map(|t| t.to_lowercase())
            .unwrap_or_default()
            == desired_type_hex
    }) {
        Ok(memo
            .memo_data
            .ok_or_else(|| anyhow::anyhow!("memo_data is missing"))?)
    } else {
        Err(anyhow::anyhow!("No memo with type: {}", memo_type))
    }
}

pub fn extract_hex_xrpl_memo(
    memos: Option<Vec<Memo>>,
    memo_type: &str,
) -> Result<String, anyhow::Error> {
    let hex_str = extract_from_xrpl_memo(memos.clone(), memo_type)?;
    let bytes = hex::decode(&hex_str)?;
    String::from_utf8(bytes).map_err(|e| e.into())
}

pub fn setup_logging(config: &Config) -> ClientInitGuard {
    let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

    let _guard = sentry::init((
        config.xrpl_relayer_sentry_dsn.to_string(),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            environment: Some(std::borrow::Cow::Owned(environment.clone())),
            traces_sample_rate: 1.0,
            ..Default::default()
        },
    ));

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_filter(LevelFilter::DEBUG);

    let sentry_layer = sentry_layer().event_filter(|metadata| match *metadata.level() {
        Level::ERROR => EventFilter::Event, // Send `error` events to Sentry
        Level::WARN => EventFilter::Event,  // Send `warn` events to Sentry
        _ => EventFilter::Breadcrumb,
    });

    let subscriber = Registry::default()
        .with(fmt_layer) // Console logging
        .with(sentry_layer); // Sentry logging

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");

    _guard
}

pub fn event_attribute(event: &WasmEvent, key: &str) -> Option<String> {
    event
        .attributes
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.value.clone())
}

pub fn parse_gas_fee_amount(
    payment_amount: &XRPLPaymentAmount,
    gas_fee_amount: String,
) -> Result<XRPLPaymentAmount, IngestorError> {
    let gas_fee_amount = match payment_amount.clone() {
        XRPLPaymentAmount::Issued(token, _) => XRPLPaymentAmount::Issued(
            XRPLToken {
                issuer: token.issuer,
                currency: token.currency,
            },
            gas_fee_amount.try_into().map_err(|_| {
                IngestorError::GenericError(
                    "Failed to parse gas fee amount as XRPLTokenAmount".to_owned(),
                )
            })?,
        ),
        XRPLPaymentAmount::Drops(_) => {
            XRPLPaymentAmount::Drops(gas_fee_amount.parse().map_err(|_| {
                IngestorError::GenericError("Failed to parse gas fee amount as u64".to_owned())
            })?)
        }
    };
    Ok(gas_fee_amount)
}

pub fn extract_memo(memos: &Option<Vec<Memo>>, memo_type: &str) -> Result<String, IngestorError> {
    extract_from_xrpl_memo(memos.clone(), memo_type).map_err(|e| {
        IngestorError::GenericError(format!("Failed to extract {} from memos: {}", memo_type, e))
    })
}

pub fn extract_and_decode_memo(
    memos: &Option<Vec<Memo>>,
    memo_type: &str,
) -> Result<String, anyhow::Error> {
    let hex_str = extract_memo(memos, memo_type)?;
    let bytes =
        hex::decode(&hex_str).with_context(|| format!("Failed to hex-decode memo {}", hex_str))?;
    String::from_utf8(bytes).with_context(|| format!("Invalid UTF-8 in memo {}", hex_str))
}

pub fn parse_payment_amount(
    payment: &PaymentTransaction,
) -> Result<XRPLPaymentAmount, IngestorError> {
    if let xrpl_api::Amount::Drops(amount) = payment.amount.clone() {
        Ok(XRPLPaymentAmount::Drops(amount.parse::<u64>().map_err(
            |_| IngestorError::GenericError("Failed to parse amount as u64".to_owned()),
        )?))
    } else if let xrpl_api::Amount::Issued(issued_amount) = payment.amount.clone() {
        Ok(XRPLPaymentAmount::Issued(
            XRPLToken {
                issuer: issued_amount.issuer.try_into().map_err(|_| {
                    IngestorError::GenericError(
                        "Failed to parse issuer as XRPLAccountId".to_owned(),
                    )
                })?,
                currency: issued_amount.currency.try_into().map_err(|_| {
                    IngestorError::GenericError(
                        "Failed to parse currency as XRPLCurrency".to_owned(),
                    )
                })?,
            },
            XRPLTokenAmount::from_str(&issued_amount.value).map_err(|_| {
                IngestorError::GenericError("Failed to parse amount as XRPLTokenAmount".to_owned())
            })?,
        ))
    } else {
        return Err(IngestorError::GenericError(
            "Payment amount must be either Drops or Issued".to_owned(),
        ));
    }
}

pub async fn xrpl_tx_from_hash(
    tx_hash: HexTxHash,
    client: &xrpl_http_client::Client,
) -> Result<Transaction, IngestorError> {
    let tx_request = TxRequest::new(&tx_hash.tx_hash_as_hex_no_prefix()).binary(false);
    client
        .call(tx_request)
        .await
        .map_err(|e| IngestorError::GenericError(format!("Failed to get transaction: {}", e)))
        .map(|res| res.tx)
}

pub fn parse_message_from_context(
    metadata: &Option<TaskMetadata>,
) -> Result<XRPLMessage, IngestorError> {
    let metadata = metadata
        .clone()
        .ok_or_else(|| IngestorError::GenericError("Verify task missing meta field".into()))?;

    let source_context = metadata.source_context.ok_or_else(|| {
        IngestorError::GenericError("Verify task missing source_context field".into())
    })?;

    let xrpl_message = source_context.get("xrpl_message").ok_or_else(|| {
        IngestorError::GenericError("Verify task missing xrpl_message in source_context".into())
    })?;

    serde_json::from_str(xrpl_message).map_err(|e| {
        IngestorError::GenericError(format!(
            "Failed to parse xrpl_message from {}: {}",
            xrpl_message, e
        ))
    })
}

pub fn setup_heartbeat(url: String) {
    tokio::spawn(async move {
        loop {
            tracing::info!("Sending heartbeat to sentry monitoring endpoint");

            match reqwest::Client::new().get(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        tracing::debug!(
                            "Successfully sent heartbeat to sentry monitoring endpoint"
                        );
                    } else {
                        tracing::error!(
                            "Failed to send heartbeat to sentry monitoring endpoint: {:?}",
                            response
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to send heartbeat to sentry monitoring endpoint: {}",
                        e
                    );
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });
}

pub fn convert_token_amount_to_drops(
    config: &Config,
    amount: f64,
    token_id: &str,
) -> Result<u64, anyhow::Error> {
    let rate = config.token_conversion_rates.get(token_id);
    if let Some(rate) = rate {
        let xrp = amount * rate;
        // WARNING: loses precision, use with caution
        Ok(Amount::xrp(&xrp.to_string()).size() as u64)
    } else {
        Err(anyhow::anyhow!(
            "No conversion rate found for token id: {}",
            token_id
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_convert_token_amount_to_drops_whole_number() {
        let mut config = Config::default();
        config.token_conversion_rates = HashMap::from([("XRP".to_string(), 1.0)]);

        let result = convert_token_amount_to_drops(&config, 123.0, "XRP").unwrap();
        assert_eq!(result, 123_000_000);
    }

    #[test]
    fn test_convert_token_amount_to_drops_with_decimals() {
        let mut config = Config::default();
        config.token_conversion_rates = HashMap::from([("XRP".to_string(), 1.0)]);

        let result = convert_token_amount_to_drops(&config, 123.456, "XRP").unwrap();
        assert_eq!(result, 123_456_000);
    }

    #[test]
    fn test_convert_token_amount_to_drops_small_value() {
        let mut config = Config::default();
        config.token_conversion_rates = HashMap::from([("XRP".to_string(), 1.0)]);

        let result = convert_token_amount_to_drops(&config, 0.000001, "XRP").unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_convert_token_amount_to_drops_max_decimals() {
        let mut config = Config::default();
        config.token_conversion_rates = HashMap::from([("XRP".to_string(), 1.0)]);

        let result = convert_token_amount_to_drops(&config, 0.123456, "XRP").unwrap();
        assert_eq!(result, 123_456);
    }

    #[test]
    fn test_convert_token_amount_to_drops_too_many_decimals_no_precision() {
        let mut config = Config::default();
        config.token_conversion_rates = HashMap::from([("XRP".to_string(), 1.0)]);

        let result = convert_token_amount_to_drops(&config, 0.1234567, "XRP").unwrap();
        assert_eq!(result, 123_456);
    }

    #[test]
    fn test_convert_token_amount_to_drops_no_rate() {
        let config = Config::default();

        let result = convert_token_amount_to_drops(&config, 0.1234567, "XRP").unwrap_err();
        assert!(result
            .to_string()
            .contains("No conversion rate found for token id: XRP"));
    }
}
