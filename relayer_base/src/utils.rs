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

pub fn extract_and_decode_memo(
    memos: &Option<Vec<Memo>>,
    memo_type: &str,
) -> Result<String, anyhow::Error> {
    let hex_str = extract_from_xrpl_memo(memos.clone(), memo_type)?;
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

// Should this be moved to the xrpl client?
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
    use std::{
        collections::{BTreeMap, HashMap},
        fs,
    };

    use xrpl_amplifier_types::types::{XRPLAccountId, XRPLCurrency};
    use xrpl_api::IssuedAmount;

    use crate::{database::MockDatabase, price_view::MockPriceView};

    use super::*;

    fn test_valid_task_parsing<T>(task_json_str: &str)
    where
        T: DeserializeOwned + serde::Serialize,
    {
        let task_json: serde_json::Value = serde_json::from_str(task_json_str).unwrap();
        let actual_task: T = serde_json::from_value(task_json.clone()).unwrap();

        let parse_result = parse_task(&task_json);
        assert!(parse_result.is_ok(), "Expected successful parsing");

        let serialized_task = serde_json::to_string(&actual_task).unwrap();
        let reserialized_json: serde_json::Value = serde_json::from_str(&serialized_task).unwrap();

        assert_eq!(reserialized_json, task_json);
    }

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

    #[test]
    fn test_parse_all_valid_tasks() {
        let valid_tasks_dir = "../testdata/xrpl_tasks/valid_tasks";
        let entries = fs::read_dir(valid_tasks_dir).expect("Failed to read valid_tasks directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .expect("Failed to get file name");

                let tasks_json = fs::read_to_string(&path)
                    .expect(&format!("Failed to load tasks from {}", path.display()));

                let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_json).expect(
                    &format!("Failed to parse tasks JSON from {}", path.display()),
                );

                for task_json in tasks {
                    let task_json_str = serde_json::to_string(&task_json)
                        .expect("Failed to serialize task back to string");

                    match file_name {
                        "VerifyTask" => test_valid_task_parsing::<VerifyTask>(&task_json_str),
                        "ExecuteTask" => test_valid_task_parsing::<ExecuteTask>(&task_json_str),
                        "GatewayTxTask" => test_valid_task_parsing::<GatewayTxTask>(&task_json_str),
                        "ConstructProofTask" => {
                            test_valid_task_parsing::<ConstructProofTask>(&task_json_str)
                        }
                        "ReactToWasmEventTask" => {
                            test_valid_task_parsing::<ReactToWasmEventTask>(&task_json_str)
                        }
                        "RefundTask" => test_valid_task_parsing::<RefundTask>(&task_json_str),
                        "ReactToRetriablePollTask" => {
                            test_valid_task_parsing::<ReactToRetriablePollTask>(&task_json_str)
                        }
                        "ReactToExpiredSigningSessionTask" => {
                            test_valid_task_parsing::<ReactToExpiredSigningSessionTask>(
                                &task_json_str,
                            )
                        }
                        _ => panic!(
                            "Unknown task file: {} - filename should match the task type name",
                            file_name
                        ),
                    }
                }
            }
        }
    }

    #[test]
    fn test_parse_invalid_tasks() {
        let tasks_json =
            std::fs::read_to_string("../testdata/xrpl_tasks/invalid_tasks/invalid_tasks.json")
                .expect("Failed to load invalid tasks");

        let tasks: Vec<serde_json::Value> =
            serde_json::from_str(&tasks_json).expect("Failed to parse invalid tasks JSON");

        for task_json in tasks {
            let task_id = task_json["id"].as_str().unwrap_or("unknown");
            let result = parse_task(&task_json);

            assert!(
                result.is_err(),
                "Expected parsing to fail for task: {}",
                task_id
            );

            if let Err(GmpApiError::InvalidResponse(_)) = result {
                // Expected error
            } else {
                panic!("Expected InvalidResponse error for task: {}", task_id);
            }
        }
    }

    #[test]
    fn test_parse_unknown_tasks() {
        let tasks_json =
            std::fs::read_to_string("../testdata/xrpl_tasks/unknown_tasks/unknown_tasks.json")
                .expect("Failed to load unknown tasks");

        let tasks: Vec<serde_json::Value> =
            serde_json::from_str(&tasks_json).expect("Failed to parse unknown tasks JSON");

        for task_json in tasks {
            let task_id = task_json["id"].as_str().unwrap_or("unknown");
            let expected_type = task_json["type"].as_str().unwrap_or("");

            let parse_result = parse_task(&task_json);
            assert!(
                parse_result.is_ok(),
                "Expected successful parsing as Unknown for task: {}",
                task_id
            );

            if let Ok(Task::Unknown(unknown_task)) = parse_result {
                assert_eq!(
                    unknown_task.common.r#type, expected_type,
                    "Type mismatch for task: {}",
                    task_id
                );
            } else {
                panic!("Expected Unknown task for: {}", task_id);
            }
        }
    }

    #[test]
    fn test_extract_from_xrpl_memo() {
        let memos = vec![Memo {
            memo_type: Some(hex::encode("test_type")),
            memo_data: Some("test_data".to_string()),
            memo_format: None,
        }];
        let maybe_memo_data = extract_from_xrpl_memo(Some(memos), "test_type");
        assert!(maybe_memo_data.is_ok());
        let memo_data = maybe_memo_data.unwrap();
        assert_eq!(memo_data, "test_data");
    }

    #[test]
    fn test_extract_from_xrpl_memo_not_hex_type() {
        let memos = vec![Memo {
            memo_type: Some("test_type".to_string()),
            memo_data: Some("test_data".to_string()),
            memo_format: None,
        }];
        let maybe_memo_data = extract_from_xrpl_memo(Some(memos), "test_type");
        assert!(maybe_memo_data.is_err());
    }

    #[test]
    fn test_extract_from_xrpl_memo_not_found() {
        let memos = vec![
            Memo {
                memo_type: None,
                memo_data: None,
                memo_format: None,
            },
            Memo {
                memo_type: Some(hex::encode("test_type")),
                memo_data: Some("test_data_2".to_string()),
                memo_format: Some("hex".to_string()),
            },
        ];
        let maybe_memo_data = extract_from_xrpl_memo(Some(memos), "test_type_2");
        assert!(maybe_memo_data.is_err());
    }

    #[test]
    fn test_extract_from_xrpl_memo_empty_list() {
        let maybe_memo_data = extract_from_xrpl_memo(Some(vec![]), "test_type");
        assert!(maybe_memo_data.is_err());
    }

    #[test]
    fn test_extract_from_xrpl_memo_missing_data() {
        let memos = vec![Memo {
            memo_type: Some(hex::encode("test_type")),
            memo_data: None,
            memo_format: None,
        }];
        let maybe_memo_data = extract_from_xrpl_memo(Some(memos), "test_type");
        assert!(maybe_memo_data.is_err());
    }

    #[test]
    fn test_extract_from_xrpl_memo_none() {
        let maybe_memo_data = extract_from_xrpl_memo(None, "test_type");
        assert!(maybe_memo_data.is_err());
    }

    #[test]
    fn test_extract_hex_xrpl_memo() {
        let memos = vec![Memo {
            memo_type: Some(hex::encode("test_type")),
            memo_data: Some(hex::encode("test_data")),
            memo_format: Some("hex".to_string()),
        }];
        let maybe_memo_hex = extract_hex_xrpl_memo(Some(memos), "test_type");
        assert!(maybe_memo_hex.is_ok());
        let memo_hex = maybe_memo_hex.unwrap();
        assert_eq!(memo_hex, "test_data");
    }

    #[test]
    fn test_extract_hex_xrpl_memo_no_hex_format() {
        let memos = vec![Memo {
            memo_type: Some("test_type".to_string()),
            memo_data: Some("test_data".to_string()),
            memo_format: None,
        }];
        let maybe_memo_hex = extract_hex_xrpl_memo(Some(memos), "test_type");
        assert!(maybe_memo_hex.is_err());
    }

    #[test]
    fn test_event_attribute() {
        let events_json = std::fs::read_to_string("../testdata/wasm_events/events.json")
            .expect("Failed to load events.json");

        let events: Vec<serde_json::Value> =
            serde_json::from_str(&events_json).expect("Failed to parse events.json");

        for event_json in events {
            let maybe_event: Result<WasmEvent, serde_json::Error> =
                serde_json::from_value(event_json.clone());
            assert!(maybe_event.is_ok());
            let actual_event = maybe_event.unwrap();
            let maybe_attribute = event_attribute(&actual_event, "poll_id");
            let attribute = maybe_attribute.unwrap();
            assert_eq!(
                attribute,
                event_json.get("attributes").unwrap()[2]
                    .get("value")
                    .unwrap()
                    .as_str()
                    .unwrap()
            );

            let maybe_attribute = event_attribute(&actual_event, "status");
            let attribute = maybe_attribute.unwrap();
            assert_eq!(
                attribute,
                event_json.get("attributes").unwrap()[3]
                    .get("value")
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
        }
    }

    #[test]
    fn test_event_attribute_not_found() {
        let events_json = std::fs::read_to_string("../testdata/wasm_events/events.json")
            .expect("Failed to load events.json");

        let events: Vec<serde_json::Value> =
            serde_json::from_str(&events_json).expect("Failed to parse events.json");

        for event_json in events {
            let maybe_event: Result<WasmEvent, serde_json::Error> =
                serde_json::from_value(event_json.clone());
            assert!(maybe_event.is_ok());
            let actual_event = maybe_event.unwrap();
            let maybe_attribute = event_attribute(&actual_event, "random_key");
            assert!(maybe_attribute.is_none());
        }
    }

    #[test]
    fn test_event_attribute_invalid_event() {
        let events_json = std::fs::read_to_string("../testdata/wasm_events/invalid_events.json")
            .expect("Failed to load events.json");

        let events: Vec<serde_json::Value> =
            serde_json::from_str(&events_json).expect("Failed to parse events.json");

        for event_json in events {
            let maybe_event: Result<WasmEvent, serde_json::Error> =
                serde_json::from_value(event_json.clone());
            assert!(maybe_event.is_err());
        }
    }

    #[test]
    fn test_parse_gas_fee_amount_drops() {
        let payment_amount = XRPLPaymentAmount::Drops(10);

        let result = parse_gas_fee_amount(&payment_amount, "500000".to_string());
        assert!(result.is_ok());
        if let Ok(XRPLPaymentAmount::Drops(amount)) = result {
            assert_eq!(amount, 500000);
        }

        let result = parse_gas_fee_amount(&payment_amount, "invalid".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gas_fee_amount_issued() {
        let token = XRPLToken {
            issuer: XRPLAccountId::from_str("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe").unwrap(),
            currency: XRPLCurrency::new("USD").unwrap(),
        };
        let payment_amount =
            XRPLPaymentAmount::Issued(token.clone(), XRPLTokenAmount::from_str("100.0").unwrap());

        let gas_fee_amount = parse_gas_fee_amount(&payment_amount, "50.0".to_string())
            .expect("Valid gas fee amount for issued tokens");
        if let XRPLPaymentAmount::Issued(result_token, amount) = gas_fee_amount {
            assert_eq!(result_token.issuer, token.issuer);
            assert_eq!(result_token.currency, token.currency);
            let expected_amount = XRPLTokenAmount::from_str("50.0").unwrap();
            assert_eq!(amount, expected_amount);
        } else {
            panic!("Expected XRPLPaymentAmount::Issued variant");
        }

        let err = parse_gas_fee_amount(&payment_amount, "invalid_amount".to_string());
        assert!(
            err.is_err(),
            "Expected error parsing invalid issued gas fee amount"
        );
    }

    #[test]
    fn test_extract_and_decode_memo() {
        let memos = vec![Memo {
            memo_type: Some(hex::encode("test_type")),
            memo_data: Some(hex::encode("test_data")),
            memo_format: Some("hex".to_string()),
        }];
        let memo_data = extract_and_decode_memo(&Some(memos), "test_type");
        assert!(memo_data.is_ok());
        let memo_data = memo_data.unwrap();
        assert_eq!(memo_data, "test_data");
    }

    #[test]
    fn test_extract_and_decode_memo_not_hex_format() {
        let memos = vec![Memo {
            memo_type: Some(hex::encode("test_type")),
            memo_data: Some("test_data".to_string()),
            memo_format: None,
        }];
        let memo_data = extract_and_decode_memo(&Some(memos), "test_type");
        assert!(memo_data.is_err());
    }

    #[test]
    fn test_extract_and_decode_memo_not_found() {
        let memos = vec![];
        let memo_data = extract_and_decode_memo(&Some(memos), "test_type");
        assert!(memo_data.is_err());
    }

    #[test]
    fn test_extract_and_decode_memo_invalid_utf8() {
        let memos = vec![Memo {
            memo_type: Some(hex::encode("test_type")),
            memo_data: Some("fffe".to_string()),
            memo_format: Some("hex".to_string()),
        }];
        let memo_data = extract_and_decode_memo(&Some(memos), "test_type");
        assert!(memo_data
            .err()
            .unwrap()
            .to_string()
            .contains("Invalid UTF-8"));
    }

    #[test]
    fn test_parse_payment_amount_drops() {
        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Drops("100".to_string()),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_ok());
        if let XRPLPaymentAmount::Drops(amount) = payment_amount.unwrap() {
            assert_eq!(amount, 100);
        } else {
            panic!("Expected XRPLPaymentAmount::Drops variant");
        }
    }

    #[test]
    fn test_parse_payment_amount_drops_invalid_amount() {
        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Drops("invalid".to_string()),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_err());

        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Drops("-100".to_string()),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_err());
    }

    #[test]
    fn test_parse_payment_amount_issued() {
        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Issued(IssuedAmount {
                value: "100.0".to_string(),
                currency: "USD".to_string(),
                issuer: "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe".to_string(),
            }),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_ok());
        if let XRPLPaymentAmount::Issued(token, amount) = payment_amount.unwrap() {
            assert_eq!(
                token.issuer,
                XRPLAccountId::from_str("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe").unwrap()
            );
            assert_eq!(token.currency, XRPLCurrency::new("USD").unwrap());
            assert_eq!(amount, XRPLTokenAmount::from_str("100.0").unwrap());
        } else {
            panic!("Expected XRPLPaymentAmount::Issued variant");
        }
    }

    #[test]
    fn test_parse_payment_amount_issued_invalid_amount() {
        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Issued(IssuedAmount {
                value: "invalid".to_string(),
                currency: "USD".to_string(),
                issuer: "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe".to_string(),
            }),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_err());
    }

    #[test]
    fn test_parse_payment_amount_issued_invalid_issuer() {
        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Issued(IssuedAmount {
                value: "100".to_string(),
                currency: "USD".to_string(),
                issuer: "random".to_string(),
            }),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_err());
    }

    #[test]
    fn test_parse_payment_amount_issued_invalid_currency() {
        let payment = PaymentTransaction {
            amount: xrpl_api::Amount::Issued(IssuedAmount {
                value: "100".to_string(),
                currency: "NONEXISTENT".to_string(),
                issuer: "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe".to_string(),
            }),
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_err());
    }

    #[test]
    fn test_parse_payment_amount_default() {
        // Default amount is 0 drops
        let payment = PaymentTransaction {
            ..Default::default()
        };
        let payment_amount = parse_payment_amount(&payment);
        assert!(payment_amount.is_ok());
        let unwrapped_payment_amount = payment_amount.unwrap();
        if let XRPLPaymentAmount::Drops(amount) = unwrapped_payment_amount {
            assert_eq!(amount, 0);
        } else {
            panic!("Expected XRPLPaymentAmount::Drops variant");
        }
    }

    #[test]
    fn test_parse_message_from_context() {
        let dummy_message : XRPLMessage = serde_json::from_str(r#"
        {
            "prover_message": {
                "tx_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "unsigned_tx_hash": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            }
        }"#).unwrap();
        let metadata = TaskMetadata {
            source_context: Some(BTreeMap::from([(
                "xrpl_message".to_string(),
                serde_json::to_string(&dummy_message).unwrap(),
            )])),
            ..Default::default()
        };
        let message_result = parse_message_from_context(&Some(metadata));
        assert!(message_result.is_ok());
        let message = message_result.unwrap();
        assert_eq!(message, dummy_message);
    }

    #[test]
    fn test_parse_message_from_context_missing_source_context() {
        let metadata = TaskMetadata {
            source_context: None,
            ..Default::default()
        };
        let message_result = parse_message_from_context(&Some(metadata));
        assert!(message_result.is_err());
        assert!(message_result
            .err()
            .unwrap()
            .to_string()
            .contains("Verify task missing source_context field"));
    }

    #[test]
    fn test_parse_message_from_context_missing_prover_message() {
        let dummy_message : XRPLMessage = serde_json::from_str(r#"
        {
            "prover_message": {
                "tx_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "unsigned_tx_hash": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            }
        }"#).unwrap();
        let metadata = TaskMetadata {
            source_context: Some(BTreeMap::from([(
                "prover_message".to_string(),
                serde_json::to_string(&dummy_message).unwrap(),
            )])),
            ..Default::default()
        };
        let message_result = parse_message_from_context(&Some(metadata));
        assert!(message_result.is_err());
        assert!(message_result
            .err()
            .unwrap()
            .to_string()
            .contains("Verify task missing xrpl_message in source_context"));
    }

    #[test]
    fn test_parse_message_from_context_failed_parsing() {
        let metadata = TaskMetadata {
            source_context: Some(BTreeMap::from([(
                "xrpl_message".to_string(),
                "invalid".to_string(),
            )])),
            ..Default::default()
        };
        let message_result = parse_message_from_context(&Some(metadata));
        assert!(message_result.is_err());
        assert!(message_result
            .err()
            .unwrap()
            .to_string()
            .contains("Failed to parse xrpl_message"));
    }

    #[test]
    fn test_message_id_from_retry_task_react_to_retriable_poll() {
        let valid_tasks_dir = "../testdata/xrpl_tasks/valid_tasks";
        let json_str =
            fs::read_to_string(format!("{}/ReactToRetriablePollTask.json", valid_tasks_dir))
                .expect("Failed to read ReactToRetriablePollTask.json");
        let tasks: Vec<ReactToRetriablePollTask> = serde_json::from_str(&json_str)
            .expect("Failed to parse JSON into Vec<ReactToRetriablePollTask>");
        // test a specific valid task
        let task = tasks
            .clone()
            .into_iter()
            .nth(1)
            .expect("Missing second task");
        let maybe_message_id = message_id_from_retry_task(Task::ReactToRetriablePoll(task));
        assert!(maybe_message_id.is_ok());
        assert_eq!(
            maybe_message_id.unwrap(),
            "5fa140ff4b90c83df9fdfdc81595bd134f41d929694eedb15cf7fd1c511e8025"
        );

        // test a specific valid task which does not have the fields we need
        let task_err = tasks.into_iter().nth(0).unwrap();
        let err = message_id_from_retry_task(Task::ReactToRetriablePoll(task_err));
        assert!(err.is_err());
    }

    #[test]
    fn test_message_id_from_retry_task_react_to_expired_signing_session() {
        let valid_tasks_dir = "../testdata/xrpl_tasks/valid_tasks";
        let json_str = fs::read_to_string(format!(
            "{}/ReactToExpiredSigningSessionTask.json",
            valid_tasks_dir
        ))
        .expect("Failed to read ReactToExpiredSigningSessionTask.json");
        let tasks: Vec<ReactToExpiredSigningSessionTask> = serde_json::from_str(&json_str)
            .expect("Failed to parse JSON into Vec<ReactToExpiredSigningSessionTask>");
        // test a specific valid task
        let task = tasks
            .clone()
            .into_iter()
            .nth(1)
            .expect("Missing second task");
        let maybe_message_id = message_id_from_retry_task(Task::ReactToExpiredSigningSession(task));
        assert!(maybe_message_id.is_ok());
        let actual_source_chain = "axelar";
        let actual_message_id =
            "0x054e170d88e181b39f638cd5da6f3c76d1a5c4f0945a4540ffddc5e13965444b-150693962";
        let actual_cc_id = CrossChainId::new(actual_source_chain, actual_message_id).unwrap();

        assert_eq!(maybe_message_id.unwrap(), actual_cc_id.to_string());

        // test a specific valid task which does not have the fields we need
        let task_err = tasks.into_iter().nth(0).unwrap();
        let err = message_id_from_retry_task(Task::ReactToExpiredSigningSession(task_err));
        assert!(err.is_err());
    }
}
