/*!
Map events from TON traces to the GMP Event. If you are adding a new transaction type,
start in parse_trace.rs and then come back here to map your output to a GMP Event.

There is probably a nicer way to encapsulate code, so adding new transactions is easier,
but this allows us not to have a tight coupling of chain parsing and GMP Events.

# TODO:
- Document conversion

*/

use crate::boc::jetton_gas_paid::JettonGasPaidMessage;
use crate::error::GasError;
use crate::parse_trace::{LogMessage, ParsedTransaction};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use num_bigint::BigUint;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    Amount, CommonEventFields, Event, EventMetadata, GatewayV2Message,
    MessageApprovedEventMetadata, MessageExecutedEventMetadata, MessageExecutionStatus,
};
use relayer_base::price_view::PriceViewTrait;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use tonlib_core::{TonAddress, TonHash};

pub fn map_message_approved(parsed_tx: &ParsedTransaction, used_gas: u64) -> Event {
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::Approved(m)) => m,
        _ => panic!("Expected LogMessage::Approved"),
    };

    let tx = &parsed_tx.transaction;
    Event::MessageApproved {
        common: CommonEventFields {
            r#type: "MESSAGE_APPROVED".to_owned(),
            event_id: tx.hash.clone(),
            meta: Some(MessageApprovedEventMetadata {
                common_meta: EventMetadata {
                    tx_id: Some(tx.hash.clone()),
                    from_address: None,
                    finalized: None,
                    source_context: None,
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                },
                command_id: None,
            }),
        },
        message: GatewayV2Message {
            message_id: msg.message_id.clone(),
            source_chain: msg.source_chain.clone(),
            source_address: msg.source_address.clone(),
            destination_address: msg.destination_address.clone(),
            payload_hash: hex::encode(msg.payload_hash),
        },
        cost: Amount {
            token_id: None,
            amount: used_gas.to_string(),
        },
    }
}

pub fn map_message_executed(parsed_tx: &ParsedTransaction, used_gas: u64) -> Event {
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::Executed(m)) => m,
        _ => panic!("Expected LogMessage::Executed"),
    };
    let tx = &parsed_tx.transaction;

    Event::MessageExecuted {
        common: CommonEventFields {
            r#type: "MESSAGE_EXECUTED".to_owned(),
            event_id: tx.hash.clone(),
            meta: Some(MessageExecutedEventMetadata {
                common_meta: EventMetadata {
                    tx_id: Some(tx.hash.clone()),
                    from_address: None,
                    finalized: None,
                    source_context: None,
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                },
                command_id: None,
                child_message_ids: None,
                revert_reason: None,
            }),
        },
        message_id: msg.message_id.clone(),
        source_chain: msg.clone().source_chain,
        status: MessageExecutionStatus::SUCCESSFUL,
        cost: Amount {
            token_id: None,
            amount: used_gas.to_string(),
        },
    }
}

pub fn map_call_contract(parsed_tx: &ParsedTransaction) -> Event {
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::CallContract(m)) => m,
        _ => panic!("Expected LogMessage::CallContract"),
    };
    let tx = &parsed_tx.transaction;

    // TODO: Make sure this is under 1000 characters
    let source_context = HashMap::from([(
        "ton_message".to_owned(),
        serde_json::to_string(msg).unwrap(),
    )]);

    let b64_payload = BASE64_STANDARD.encode(
        hex::decode(&msg.payload)
            .map_err(|e| IngestorError::GenericError(format!("Failed to decode payload: {}", e)))
            .unwrap(), // We should be safe here to unwrap
    );

    Event::Call {
        common: CommonEventFields {
            r#type: "CALL".to_owned(),
            event_id: tx.hash.clone(),
            meta: Some(EventMetadata {
                tx_id: Some(tx.hash.clone()),
                from_address: None,
                finalized: None,
                source_context: Some(source_context),
                timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            }),
        },
        message: GatewayV2Message {
            message_id: parsed_tx.message_id.clone().unwrap(),
            source_chain: "ton2".to_string(), // TODO: Do not hardcode
            source_address: msg.source_address.to_hex(),
            destination_address: msg.destination_address.to_string(),
            payload_hash: BASE64_STANDARD.encode(msg.payload_hash),
        },
        destination_chain: msg.destination_chain.clone(),
        payload: b64_payload,
    }
}

pub fn map_native_gas_paid(parsed_tx: &ParsedTransaction) -> Event {
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::NativeGasPaid(m)) => m,
        _ => panic!("Expected LogMessage::NativeGasPaid"),
    };
    let tx = &parsed_tx.transaction;

    Event::GasCredit {
        common: CommonEventFields {
            r#type: "GAS_CREDIT".to_owned(),
            event_id: format!("{}-gas", tx.hash.clone()),
            meta: Some(EventMetadata {
                tx_id: Some(tx.hash.clone()),
                from_address: None,
                finalized: None,
                source_context: None,
                timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            }),
        },
        message_id: parsed_tx.message_id.clone().unwrap(),
        refund_address: msg.refund_address.to_hex(),
        payment: Amount {
            token_id: None,
            amount: msg.msg_value.to_string(),
        },
    }
}

pub fn map_native_gas_added(parsed_tx: &ParsedTransaction) -> Event {
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::NativeGasAdded(m)) => m,
        _ => panic!("Expected LogMessage::NativeGasPaid"),
    };
    let tx = &parsed_tx.transaction;

    Event::GasCredit {
        common: CommonEventFields {
            r#type: "GAS_CREDIT".to_owned(),
            event_id: format!("{}-gas-added", tx.hash.clone()),
            meta: Some(EventMetadata {
                tx_id: Some(tx.hash.clone()),
                from_address: None,
                finalized: None,
                source_context: None,
                timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            }),
        },
        message_id: parsed_tx.message_id.clone().unwrap(),
        refund_address: msg.refund_address.to_hex(),
        payment: Amount {
            token_id: None,
            amount: msg.msg_value.to_string(),
        },
    }
}

pub fn map_message_native_gas_refunded(parsed_tx: &ParsedTransaction, used_gas: u64) -> Event {
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::NativeGasRefunded(m)) => m,
        _ => panic!("Expected LogMessage::NativeGasPaid"),
    };
    let tx = &parsed_tx.transaction;

    Event::GasRefunded {
        common: CommonEventFields {
            r#type: "GAS_REFUNDED".to_owned(),
            event_id: tx.hash.clone(),
            meta: None,
        },
        message_id: parsed_tx.message_id.clone().unwrap(),
        recipient_address: msg.address.to_hex(),
        refunded_amount: Amount {
            token_id: None,
            amount: msg.amount.to_string(),
        },
        cost: Amount {
            token_id: None,
            amount: used_gas.to_string(),
        },
    }
}

pub async fn map_jetton_gas_paid<PV>(
    parsed_tx: &ParsedTransaction,
    price_view: &PV,
) -> Result<Event, GasError>
where
    PV: PriceViewTrait,
{
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::JettonGasPaid(m)) => m,
        _ => panic!("Expected LogMessage::JettonGasPaid"),
    };
    let tx = &parsed_tx.transaction;

    let msg_value = convert_jetton_to_native(&msg.minter, &msg.amount, price_view).await?;

    Ok(Event::GasCredit {
        common: CommonEventFields {
            r#type: "GAS_CREDIT".to_owned(),
            event_id: format!("{}-gas", tx.hash.clone()),
            meta: Some(EventMetadata {
                tx_id: Some(tx.hash.clone()),
                from_address: None,
                finalized: None,
                source_context: None,
                timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            }),
        },
        message_id: parsed_tx.message_id.clone().unwrap(),
        refund_address: msg.refund_address.to_hex(),
        payment: Amount {
            token_id: None,
            amount: msg_value.to_string(),
        },
    })
}

pub async fn map_jetton_gas_added<PV>(
    parsed_tx: &ParsedTransaction,
    price_view: &PV,
) -> Result<Event, GasError>
where
    PV: PriceViewTrait,
{
    let msg = match &parsed_tx.log_message {
        Some(LogMessage::JettonGasAdded(m)) => m,
        _ => panic!("Expected LogMessage::NativeGasPaid"),
    };
    let tx = &parsed_tx.transaction;
    
    let msg_value = convert_jetton_to_native(&msg.minter, &msg.amount, price_view).await?;

    Ok(Event::GasCredit {
        common: CommonEventFields {
            r#type: "GAS_CREDIT".to_owned(),
            event_id: format!("{}-gas-added", tx.hash.clone()),
            meta: Some(EventMetadata {
                tx_id: Some(tx.hash.clone()),
                from_address: None,
                finalized: None,
                source_context: None,
                timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            }),
        },
        message_id: parsed_tx.message_id.clone().unwrap(),
        refund_address: msg.refund_address.to_hex(),
        payment: Amount {
            token_id: None,
            amount: msg_value.to_string(),
        },
    })
}


async fn convert_jetton_to_native<PV>(
    minter: &TonAddress,
    amount: &BigUint,
    price_view: &PV,
) -> Result<BigUint, GasError>
where
    PV: PriceViewTrait,
{
    let minter = minter.to_hex();

    let coin_pair = format!("{}/USD", minter);
    let coin_to_usd = price_view
        .get_price(&coin_pair)
        .await
        .map_err(|err| GasError::ConversionError(err.to_string()))?;
    let ton_to_usd = price_view
        .get_price("TON/USD")
        .await
        .map_err(|err| GasError::ConversionError(err.to_string()))?;

    let amount = Decimal::from_str(&amount.to_string()).unwrap();
    let result = amount * coin_to_usd / ton_to_usd;
    let result = result.round();

    BigUint::from_str(&result.to_string()).map_err(|err| GasError::ConversionError(err.to_string()))
}

#[cfg(test)]
mod tests {
    use std::cmp::min;
    use crate::boc::jetton_gas_paid::JettonGasPaidMessage;
    use crate::event_mappers::{
        convert_jetton_to_native, map_call_contract, map_jetton_gas_paid, map_message_approved,
        map_message_executed, map_native_gas_added, map_native_gas_paid,
    };
    use crate::parse_trace::ParseTrace;
    use mockall::predicate::*;
    use num_bigint::BigUint;
    use relayer_base::database::PostgresDB;
    use relayer_base::gmp_api::gmp_types::{Event, MessageExecutionStatus};
    use relayer_base::price_view::MockPriceView;
    use relayer_base::ton_types::{Trace, TracesResponse, TracesResponseRest};
    use rust_decimal::Decimal;
    use std::fs;
    use std::str::FromStr;
    use tonlib_core::TonAddress;

    fn fixture_traces() -> Vec<Trace> {
        let file_path = "tests/data/v3_traces.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let rest: TracesResponseRest =
            serde_json::from_str(&body).expect("Failed to deserialize test transaction data");

        TracesResponse::from(rest).traces
    }

    #[test]
    fn test_map_message_approved() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[0].clone()).unwrap();

        let event = map_message_approved(&trace_transactions.message_approved[0], 123);

        match event {
            Event::MessageApproved {
                common,
                message,
                cost,
            } => {
                assert_eq!(
                    message.message_id,
                    "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58-1"
                );
                assert_eq!(message.source_chain, "avalanche-fuji");
                assert_eq!(
                    message.payload_hash,
                    "9e01c423ca440c5ec2beecc9d0a152b54fc8e7a416c931b7089eaf221605af17"
                );
                assert_eq!(cost.amount, "123");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(meta.common_meta.tx_id.as_deref(), Some("aa1"));
            }
            _ => panic!("Expected MessageApproved event"),
        }
    }

    #[test]
    fn test_map_message_executed() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[0].clone()).unwrap();

        let event = map_message_executed(&trace_transactions.executed[0], 321);

        match event {
            Event::MessageExecuted {
                common,
                message_id,
                source_chain,
                status,
                cost,
            } => {
                assert_eq!(
                    message_id,
                    "0xf2b741fb0b2c2fcf92aca82395bc65dab4dd8239a12f366d6045755e0b02c2a2-1"
                );
                assert_eq!(source_chain, "avalanche-fuji");
                assert_eq!(status, MessageExecutionStatus::SUCCESSFUL);
                assert_eq!(cost.amount, "321");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(meta.common_meta.tx_id.as_deref(), Some("aa4"));
                assert_eq!(meta.revert_reason.as_deref(), None);
            }
            _ => panic!("Expected MessageExecuted event"),
        }
    }

    #[test]
    fn test_map_call_contract() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[1].clone()).unwrap();

        let event = map_call_contract(&trace_transactions.call_contract[0]);

        match event {
            Event::Call {
                common,
                message,
                destination_chain,
                payload,
            } => {
                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(payload, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gUmVsYXllciEAAAAAAAAAAAAAAAAA");
                assert_eq!(
                    message.message_id,
                    "0xd60ccda763591b1af5a1771f0913a6851174ef161da21ed7e750a0240db1fd03"
                );
                assert_eq!(message.source_chain, "ton2");
                assert_eq!(
                    message.payload_hash,
                    "rqZSQ2cAD7SgqiCx1PY9qtHtnp33Fj8jCWc2EPLzfUs="
                );
                assert_eq!(
                    message.source_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("1gzNp2NZGxr1oXcfCROmhRF07xYdoh7X51CgJA2x/QM=")
                );
            }
            _ => panic!("Expected CallContract event"),
        }
    }

    #[test]
    fn test_map_gas_credit() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[4].clone()).unwrap();

        let event = map_native_gas_paid(&trace_transactions.gas_credit[0]);

        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(
                    message_id,
                    "0xcaee5a2c0eac0e3dd666d440934871c74e21c2d56be38a9fcb8c7905c449172b"
                );
                assert_eq!(
                    refund_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(payment.amount, "28846800");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("Ptv+ldOh9sTQOvwx23nPD8t6iGmm2RZVgUBXBk/jyrU=")
                );
            }
            _ => panic!("Expected GasCredit event"),
        }
    }

    #[test]
    fn test_map_native_gas_added() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[5].clone()).unwrap();

        let event = map_native_gas_added(&trace_transactions.gas_added[0]);

        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(
                    message_id,
                    "0x0e6f759f68edb972cc1c5ac28ae44a026567c39d0a67d71de90978a12106a6ba"
                );
                assert_eq!(
                    refund_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(payment.amount, "299338000");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("hlbJSt6b0kkNh0We16gyIxE5WyDRDltaKIYOmfEZtAs=")
                );
            }
            _ => panic!("Expected GasCredit event"),
        }
    }

    #[test]
    fn test_map_message_native_gas_refunded() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[7].clone()).unwrap();

        let event =
            super::map_message_native_gas_refunded(&trace_transactions.gas_refunded[0], 456);

        match event {
            Event::GasRefunded {
                common,
                message_id,
                recipient_address,
                refunded_amount,
                cost,
            } => {
                assert_eq!(
                    message_id,
                    "0xeb065d9d930349d0643b946d961ec600f80d5e5f30ab01df6f136243ee5035c2"
                );
                assert_eq!(
                    recipient_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(refunded_amount.amount, "266907599");
                assert_eq!(cost.amount, "456");
                assert_eq!(refunded_amount.token_id.as_deref(), None);
                assert_eq!(cost.token_id.as_deref(), None);
                assert!(common.meta.is_none());
            }
            _ => panic!("Expected GasRefunded event"),
        }
    }

    #[tokio::test]
    async fn test_convert_jetton_to_native() {
        let mut price_view: MockPriceView<PostgresDB> = MockPriceView::new();
        price_view
            .expect_get_price()
            .with(eq(
                "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7/USD",
            ))
            .returning(|_| Ok(Decimal::from_str(&"0.5").unwrap()));
        price_view
            .expect_get_price()
            .with(eq("TON/USD"))
            .returning(|_| Ok(Decimal::from_str(&"3").unwrap()));

        let minter = TonAddress::from_base64_url("EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC")
            .unwrap();
        let amount = BigUint::from(1000u32);
        let result = convert_jetton_to_native(&minter, &amount, &price_view).await.unwrap();
        assert_eq!(result, BigUint::from(167u32));
    }

    #[tokio::test]
    async fn test_map_jetton_gas_paid() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[9].clone()).unwrap();
        let mut price_view: MockPriceView<PostgresDB> = MockPriceView::new();
        price_view
            .expect_get_price()
            .with(eq(
                "0:1962e375dcf78f97880e9bec4f63e1afe683b4abdd8855d366014c05ff1160e9/USD",
            ))
            .returning(|_| Ok(Decimal::from_str(&"0.5").unwrap()));
        price_view
            .expect_get_price()
            .with(eq("TON/USD"))
            .returning(|_| Ok(Decimal::from_str(&"3").unwrap()));

        let event = map_jetton_gas_paid(&trace_transactions.gas_credit[0], &price_view)
            .await
            .unwrap();

        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(
                    message_id,
                    "0xd59014fd585eed8bee519c40d93be23a991fdb7d68a41eb7ad678dc40510e65d"
                );
                assert_eq!(
                    refund_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(payment.amount, "166667");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("/OxewvVQHSEhT6pz1L/et2BKJC7avRCYEx0FbUWPEuo=")
                );
            }
            _ => panic!("Expected GasCredit event"),
        }
    }
}
