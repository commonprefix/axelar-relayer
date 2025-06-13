use std::str::FromStr;

use anyhow::Context;
use axelar_wasm_std::msg_id::HexTxHash;
use redis::{Commands, SetExpiry, SetOptions};
use router_api::CrossChainId;
use rust_decimal::Decimal;
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
use xrpl_api::{Memo, PaymentTransaction, Transaction, TxRequest};

use crate::{
    config::Config,
    error::{GmpApiError, IngestorError},
    gmp_api::gmp_types::{
        CommonTaskFields, ConstructProofTask, ExecuteTask, GatewayTxTask,
        ReactToExpiredSigningSessionTask, ReactToRetriablePollTask, ReactToWasmEventTask,
        RefundTask, Task, TaskMetadata, UnknownTask, VerifyTask, WasmEvent,
    },
    price_view::PriceViewTrait,
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
        "REACT_TO_RETRIABLE_POLL" => {
            let task: ReactToRetriablePollTask = parse_as(task_json)?;
            Ok(Task::ReactToRetriablePoll(task))
        }
        "REACT_TO_EXPIRED_SIGNING_SESSION" => {
            let task: ReactToExpiredSigningSessionTask = parse_as(task_json)?;
            Ok(Task::ReactToExpiredSigningSession(task))
        }
        _ => {
            let task: UnknownTask = parse_as(task_json)?;
            Ok(Task::Unknown(task))
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

pub fn setup_heartbeat(url: String, redis_pool: r2d2::Pool<redis::Client>) {
    tokio::spawn(async move {
        loop {
            tracing::info!("Writing heartbeat to DB");
            let mut redis_conn = redis_pool.get().unwrap();
            let set_opts = SetOptions::default().with_expiration(SetExpiry::EX(30));
            let result: redis::RedisResult<()> = redis_conn.set_options(url.clone(), "1", set_opts);
            if let Err(e) = result {
                tracing::error!("Failed to write heartbeat: {}", e);
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
        }
    });
}

pub async fn convert_token_amount_to_drops<T>(
    config: &Config,
    amount: Decimal,
    token_id: &str,
    price_view: &T,
) -> Result<String, anyhow::Error>
where
    T: PriceViewTrait,
{
    let token_symbol = config
        .deployed_tokens
        .get(token_id)
        .ok_or_else(|| anyhow::anyhow!("Token id {} not found in deployed tokens", token_id))?;

    let price = price_view
        .get_price(&format!("{}/XRP", token_symbol))
        .await?;

    let xrp = amount * price;
    let drops = xrp * Decimal::from(1_000_000);

    if drops.normalize().scale() > 0 {
        warn!("Losing precision, drops have decimal points: {}", drops);
    }
    Ok(drops.trunc().to_string())
}

pub fn message_id_from_retry_task(task: Task) -> Result<String, anyhow::Error> {
    match task {
        Task::ReactToRetriablePoll(task) => {
            let payload: Value = serde_json::from_str(&task.task.request_payload)?;
            let tx_id_value = payload
                .get("verify_messages")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("add_gas_message"))
                .and_then(|v| v.get("tx_id"))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Failed to extract tx_id from verify_messages[0].add_gas_message.tx_id"
                    )
                })?;
            let tx_id_str = tx_id_value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("tx_id is not a string: {:?}", tx_id_value))?
                .to_owned();
            Ok(tx_id_str)
        }
        Task::ReactToExpiredSigningSession(task) => {
            let payload: Value = serde_json::from_str(&task.task.request_payload)?;
            let construct_proof = payload
                .get("construct_proof")
                .ok_or_else(|| anyhow::anyhow!("construct_proof is missing"))?;
            let cc_id = construct_proof
                .get("cc_id")
                .ok_or_else(|| anyhow::anyhow!("cc_id is missing"))?;
            let message_id = cc_id
                .get("message_id")
                .ok_or_else(|| anyhow::anyhow!("message_id is missing"))?
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("message_id is not a string"))?;
            let source_chain = cc_id
                .get("source_chain")
                .ok_or_else(|| anyhow::anyhow!("source_chain is missing"))?
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("source_chain is not a string"))?;
            let cc_id = CrossChainId::new(source_chain, message_id)
                .map_err(|e| anyhow::anyhow!("Failed to create CrossChainId: {}", e))?;
            let cc_id_str = cc_id.to_string();
            Ok(cc_id_str)
        }
        _ => Err(anyhow::anyhow!("Irrelevant task")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{database::MockDatabase, price_view::MockPriceView};

    use super::*;

    #[tokio::test]
    async fn test_convert_token_amount_to_drops_whole_number() {
        let config = Config {
            deployed_tokens: HashMap::from([("XRP".to_string(), "XRP".to_string())]),
            ..Default::default()
        };

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.5").unwrap()));
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_str("123.0").unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap();
        assert_eq!(result, "184500000");
    }

    #[tokio::test]
    async fn test_convert_token_amount_to_drops_with_decimals() {
        let config = Config {
            deployed_tokens: HashMap::from([("XRP".to_string(), "XRP".to_string())]),
            ..Default::default()
        };

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.5").unwrap()));
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_str("123.456").unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap();
        assert_eq!(result, "185184000");
    }

    #[tokio::test]
    async fn test_convert_token_amount_to_drops_small_value() {
        let config = Config {
            deployed_tokens: HashMap::from([("XRP".to_string(), "XRP".to_string())]),
            ..Default::default()
        };

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.0").unwrap()));
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_str("0.000001").unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap();
        assert_eq!(result, "1");
    }

    #[tokio::test]
    async fn test_convert_token_amount_to_drops_max_decimals() {
        let config = Config {
            deployed_tokens: HashMap::from([("XRP".to_string(), "XRP".to_string())]),
            ..Default::default()
        };

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.0").unwrap()));
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_str("0.123456").unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap();
        assert_eq!(result, "123456");
    }

    #[tokio::test]
    async fn test_convert_token_amount_to_drops_too_many_decimals_no_precision() {
        let config = Config {
            deployed_tokens: HashMap::from([("XRP".to_string(), "XRP".to_string())]),
            ..Default::default()
        };

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.0").unwrap()));
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_str("0.1234567").unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap();
        assert_eq!(result, "123456");
    }

    #[tokio::test]
    async fn test_convert_token_amount_to_drops_no_rate() {
        let config = Config::default();

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.0").unwrap()));
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_str("0.1234567").unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap_err();
        assert!(result
            .to_string()
            .contains("Token id XRP not found in deployed tokens"));
    }

    #[tokio::test]
    async fn test_convert_xrpl_token_amount_to_drops() {
        let config = Config {
            deployed_tokens: HashMap::from([("XRP".to_string(), "XRP".to_string())]),
            ..Default::default()
        };

        let mut price_view = MockPriceView::<MockDatabase>::new();
        price_view
            .expect_get_price()
            .returning(|_| Ok(Decimal::from_str("1.0").unwrap()));

        let token_amount = XRPLTokenAmount::from_str("123.456").unwrap();
        let result = convert_token_amount_to_drops(
            &config,
            Decimal::from_scientific(&token_amount.to_string()).unwrap(),
            "XRP",
            &price_view,
        )
        .await
        .unwrap();
        assert_eq!(result, "123456000");
    }
}
