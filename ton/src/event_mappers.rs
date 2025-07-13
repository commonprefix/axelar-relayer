/*!
Map events from TON traces to the GMP Event. If you are adding a new transaction type,
start in parse_trace.rs and then come back here to map your output to a GMP Event.

There is probably a nicer way to encapsulate code so adding new transactions is easier,
but this allows us not to have to tightly couple chain parsing and GMP Events.
*/

use crate::parse_trace::{LogMessage, ParsedTransaction};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    Amount, CommonEventFields, Event, EventMetadata, GatewayV2Message,
    MessageApprovedEventMetadata, MessageExecutedEventMetadata, MessageExecutionStatus,
};
use std::collections::HashMap;

pub fn map_message_approved(parsed_tx: &ParsedTransaction) -> Event {
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
            amount: "0".to_string(),
        },
    }
}

pub fn map_message_executed(parsed_tx: &ParsedTransaction) -> Event {
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
            amount: "0".to_string(),
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

pub fn map_gas_credit(parsed_tx: &ParsedTransaction) -> Event {
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
        refund_address: msg.sender.to_base64_url(),
        payment: Amount {
            token_id: None,
            amount: msg.msg_value.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::event_mappers::{
        map_call_contract, map_gas_credit, map_message_approved, map_message_executed,
    };
    use crate::parse_trace::ParseTrace;
    use relayer_base::gmp_api::gmp_types::{
        Event,
        MessageExecutionStatus,
    };
    use relayer_base::ton_types::{Trace, TracesResponse};
    use std::fs;

    fn fixture_traces() -> Vec<Trace> {
        let file_path = "tests/data/v3_traces.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let res: TracesResponse =
            serde_json::from_str(&body).expect("Failed to deserialize test transaction data");

        res.traces
    }

    #[test]
    fn test_map_message_approved() {
        let traces = fixture_traces();
        let trace_transactions =
            crate::parse_trace::TraceTransactions::from_trace(traces[0].clone()).unwrap();

        let event = map_message_approved(&trace_transactions.message_approved[0]);

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
                assert_eq!(cost.amount, "0");
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

        let event = map_message_executed(&trace_transactions.executed[0]);

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
                assert_eq!(cost.amount, "0");
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

        let event = map_gas_credit(&trace_transactions.gas_credit[0]);

        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(
                    message_id,
                    "0x3edbfe95d3a1f6c4d03afc31db79cf0fcb7a8869a6d91655814057064fe3cab5"
                );
                assert_eq!(
                    refund_address,
                    "EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC"
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
}
