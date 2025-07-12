/*!

# TODO:
- source_context has a limit of 1000 characters, make sure we never exceed it
- Handle all errors, no unwraps() on reading from API
- Move handlers to a decorator pattern (?)
*/

use std::collections::HashMap;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use router_api::CrossChainId;
use relayer_base::error::GmpApiError::GenericError;
use crate::boc_cc_message::TonCCMessage;
use crate::boc_nullified_message::NullifiedSuccessfullyMessage;
use crate::ton_op_codes::{OP_CALL_CONTRACT, OP_GATEWAY_EXECUTE, OP_MESSAGE_APPROVED, OP_NULLIFIED_SUCCESSFULLY};
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    Amount, CommonEventFields, ConstructProofTask, Event, EventMetadata, GatewayV2Message,
    MessageApprovedEventMetadata, MessageExecutedEventMetadata, MessageExecutionStatus,
    ReactToWasmEventTask, RetryTask, VerifyTask,
};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::payload_cache::{PayloadCacheTrait};
use relayer_base::subscriber::ChainTransaction;
use relayer_base::ton_types::Transaction;
use crate::boc_call_contract::CallContractMessage;

pub struct TONIngestor {
}

impl TONIngestor {
    pub fn new() -> Self {
        Self {}
    }

    fn body_if_call_contract(&self, tx: &Transaction) -> Option<String> {
        let has_out_empty_dest = tx
            .out_msgs
            .iter()
            .any(|msg| msg.destination.as_deref().unwrap_or("").is_empty());

        let op_code = format!("0x{:08x}", OP_CALL_CONTRACT);
        let in_msg_opcode = tx.in_msg.as_ref().map_or(false, |msg| {
            let deref = msg.opcode.as_deref();
            deref.is_some() && msg.opcode.as_deref().unwrap() == op_code.clone()
        });

        if has_out_empty_dest && in_msg_opcode {
            tx.out_msgs
                .iter()
                .find(|msg| msg.destination.as_deref().unwrap_or("").is_empty())
                .map(|msg| msg.message_content.body.clone())
        } else {
            None
        }
    }


    fn body_if_approved(&self, tx: &Transaction) -> Option<String> {
        let has_out_empty_dest = tx
            .out_msgs
            .iter()
            .any(|msg| msg.destination.as_deref().unwrap_or("").is_empty());

        let message_approved = format!("0x{:08x}", OP_MESSAGE_APPROVED);
        let in_msg_opcode_d = tx.in_msg.as_ref().map_or(false, |msg| {
            let deref = msg.opcode.as_deref();
            deref.is_some() && msg.opcode.as_deref().unwrap() == message_approved.clone()
        });

        if has_out_empty_dest && in_msg_opcode_d {
            tx.out_msgs
                .iter()
                .find(|msg| msg.destination.as_deref().unwrap_or("").is_empty())
                .map(|msg| msg.message_content.body.clone())
        } else {
            None
        }
    }

    fn body_if_executed(&self, tx: &Transaction) -> Option<String> {
        let has_out_empty_dest = tx
            .out_msgs
            .iter()
            .any(|msg| msg.destination.as_deref().unwrap_or("").is_empty());

        let gateway_execute = format!("0x{:08x}", OP_GATEWAY_EXECUTE);
        let out_msg_opcode = tx.out_msgs.iter().any(|msg| {
            let deref = msg.opcode.as_deref();
            deref.is_some() && msg.opcode.as_deref().unwrap() == gateway_execute.clone()
        });

        if has_out_empty_dest && out_msg_opcode {
            tx.in_msg
                .iter()
                .find(|msg| {
                    msg.opcode.as_deref().unwrap_or("")
                        == format!("0x{:08x}", OP_NULLIFIED_SUCCESSFULLY)
                })
                .map(|msg| msg.message_content.body.clone())
        } else {
            None
        }
    }

    async fn handle_executed(
        &self,
        tx: Transaction,
        body: &str,
    ) -> Result<Vec<Event>, IngestorError> {
        let message = NullifiedSuccessfullyMessage::from_boc_b64(&body)
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let event = Event::MessageExecuted {
            common: CommonEventFields {
                r#type: "MESSAGE_EXECUTED".to_owned(),
                event_id: tx.hash.clone(),
                meta: Some(MessageExecutedEventMetadata {
                    common_meta: EventMetadata {
                        tx_id: tx.hash.into(),
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
            message_id: message.message_id,
            source_chain: message.source_chain,
            status: MessageExecutionStatus::SUCCESSFUL,
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        };

        Ok(vec![event])
    }

    async fn handle_approved(
        &self,
        tx: Transaction,
        body: &str,
    ) -> Result<Vec<Event>, IngestorError> {
        let log = TonCCMessage::from_boc_b64(&body).unwrap();

        let cc_id = CrossChainId::new(
            log.source_chain.clone(),
            log.message_id.clone()
        ).map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let event = Event::MessageApproved {
            common: CommonEventFields {
                r#type: "MESSAGE_APPROVED".to_owned(),
                event_id: tx.hash.clone(),
                meta: Some(MessageApprovedEventMetadata {
                    common_meta: EventMetadata {
                        tx_id: tx.hash.into(),
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
                message_id: log.message_id,
                source_chain: log.source_chain,
                source_address: log.source_address,
                destination_address: log.destination_address,
                payload_hash: hex::encode(log.payload_hash)
            },
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        };

        Ok(vec![event])
    }

    async fn handle_call_contract(&self, tx: Transaction, body: &str) -> Result<Vec<Event>, IngestorError> {
        let hash = BASE64_STANDARD.decode(&tx.hash).map_err(|e| GenericError(e.to_string())).unwrap();
        let hash = hex::encode(hash);

        let call_contract = CallContractMessage::from_boc_b64(&body).unwrap();
        let source_context = HashMap::from([(
            "ton_message".to_owned(),
            serde_json::to_string(&call_contract).unwrap(),
        )]);

        let b64_payload = BASE64_STANDARD.encode(
            hex::decode(call_contract.payload).map_err(|e| {
                IngestorError::GenericError(format!("Failed to decode payload: {}", e))
            })?,
        );

        let event = Event::Call {
            common: CommonEventFields {
                r#type: "CALL".to_owned(),
                event_id: tx.hash.clone(),
                meta: Some(EventMetadata {
                    tx_id: Some(tx.hash.clone()),
                    from_address: None,
                    finalized: None,
                    source_context: Some(source_context),
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message: GatewayV2Message {
                message_id: format!("0x{}", hash.to_lowercase()),
                source_chain: "ton2".to_string(), // TODO: Do not hardcode
                source_address: call_contract.source_address.to_hex(),
                destination_address: call_contract.destination_address.to_string(),
                payload_hash: BASE64_STANDARD.encode(call_contract.payload_hash),
            },
            destination_chain: call_contract.destination_chain,
            payload: b64_payload,
        };

        let gas_credit = Event::GasCredit {
            common: CommonEventFields {
                r#type: "GAS_CREDIT".to_owned(),
                event_id: format!("{}-gas", tx.hash.clone()),
                meta: Some(EventMetadata {
                    tx_id: Some(tx.hash.clone()),
                    from_address: None,
                    finalized: None,
                    source_context: None,
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message_id: format!("0x{}", hash.to_lowercase()),
            refund_address: call_contract.source_address.to_base64_url(),
            payment: Amount {
                token_id: None,
                amount: "5000000000".to_string(),
            },
        };

        Ok(vec![event, gas_credit])

    }
}

impl IngestorTrait for TONIngestor {
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        println!("handle_verify: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_transaction(&self, tx: ChainTransaction) -> Result<Vec<Event>, IngestorError> {
        let ChainTransaction::TON(tx) = tx else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!(
                "{:?}",
                tx
            )));
        };

        if let Some(body) = self.body_if_executed(&tx) {
            return self.handle_executed(tx, &body).await;
        }

        if let Some(body) = self.body_if_approved(&tx) {
            return self.handle_approved(tx, &body).await;
        }

        if let Some(body) = self.body_if_call_contract(&tx) {
            return self.handle_call_contract(tx, &body).await;
        }


        Ok(vec![])
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        println!("handle_wasm_event: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        println!("handle_construct_proof: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        println!("handle_retriable_task: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::ingestor::TONIngestor;
    use relayer_base::gmp_api::gmp_types::{Event, MessageExecutionStatus};
    use relayer_base::ton_types::{Transaction, TransactionsResponse};
    use std::fs;

    fn fixture_transactions() -> Vec<Transaction> {
        let file_path = "tests/data/v3_transactions.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let transactions_response: TransactionsResponse =
            serde_json::from_str(&body).expect("Failed to deserialize test transaction data");

        transactions_response.transactions
    }

    #[tokio::test]
    async fn test_body_if_approved_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];
        let ingestor = TONIngestor::new();
        let approved_body = ingestor.body_if_approved(tx);

        assert!(
            approved_body.is_some(),
            "Expected transaction to be an approved message"
        );
    }

    #[tokio::test]
    async fn test_is_not_approved_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];
        let ingestor = TONIngestor::new();

        let approved_body = ingestor.body_if_approved(tx);

        assert!(
            approved_body.is_none(),
            "Expected the transaction not to be an approved message"
        );
    }

    #[tokio::test]
    async fn test_body_if_executed_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];
        let ingestor = TONIngestor::new();

        let approved_body = ingestor.body_if_executed(tx);

        assert_eq!(approved_body.unwrap(), "te6cckECCAEAAV0ABIsAAAAFgBIHqwhg5lg4ES2+GWhwn4EVgGvmj7MoTr6OJXwhB8Byr9KMj8CFtEqwFmUtJVgVpEqk3ftJTCRWAx2zya/xlWvwAQIDBACIMHhmMmI3NDFmYjBiMmMyZmNmOTJhY2E4MjM5NWJjNjVkYWI0ZGQ4MjM5YTEyZjM2NmQ2MDQ1NzU1ZTBiMDJjMmEyLTEAHGF2YWxhbmNoZS1mdWppAFQweGQ3MDY3QWUzQzM1OWU4Mzc4OTBiMjhCN0JEMGQyMDg0Q2ZEZjQ5YjUDAAUGBwDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD0hlbGxvIGZyb20gdG9uIQAAAAAAAAAAAAAAAAAAAAAAAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMtlN//0=");
    }

    #[tokio::test]
    async fn test_is_not_executed_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];
        let ingestor = TONIngestor::new();
        let approved_body = ingestor.body_if_executed(tx);

        assert!(
            approved_body.is_none(),
            "Expected transaction not to be execute message"
        );
    }

    #[tokio::test]
    async fn test_body_if_call_contract_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[4];
        let ingestor = TONIngestor::new();

        let body = ingestor.body_if_call_contract(tx).unwrap();

        assert_eq!(body, "te6cckEBBAEA5QADg4AcPMZ9bgNiMWiFLuLZ3ODT3Qj2rbcRiS/f1NA9opZaWPXUykhs4AH2lBVEFjqex7VaPbPTvuLH5GEs5sIeXm+pcAECAwAcYXZhbGFuY2hlLWZ1amkAVDB4ZDcwNjdBZTNDMzU5ZTgzNzg5MGIyOEI3QkQwZDIwODRDZkRmNDliNQDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gUmVsYXllciEAAAAAAAAAAAAAAAAAne0F4Q==");
    }

    #[tokio::test]
    async fn test_is_not_call_contract_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];
        let ingestor = TONIngestor::new();
        let approved_body = ingestor.body_if_call_contract(tx);

        assert!(
            approved_body.is_none(),
            "Expected transaction not to be call contract message"
        );
    }

    #[tokio::test]
    async fn test_handle_executed() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];
        let ingestor = TONIngestor::new();
        let body = ingestor.body_if_executed(tx);

        let res = ingestor
            .handle_executed(tx.clone(), &body.unwrap())
            .await
            .unwrap();
        assert_eq!(res.len(), 1);
        let event = &res[0];

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
                assert_eq!(status, &MessageExecutionStatus::SUCCESSFUL);
                assert_eq!(cost.amount, "0");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.common_meta.tx_id.as_deref(),
                    Some("5vCHizXg+dERKuBuXsB9FCSu7soTxQTVc6zta0Qn60M=")
                );
                assert_eq!(meta.revert_reason.as_deref(), None);
            }
            _ => panic!("Expected MessageExecuted event"),
        }
    }

    #[tokio::test]
    async fn test_handle_approved() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];


        let ingestor = TONIngestor::new();

        let body = ingestor.body_if_approved(tx);

        let res = ingestor
            .handle_approved(tx.clone(), &body.unwrap())
            .await
            .unwrap();
        assert_eq!(res.len(), 1);
        let event = &res[0];

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
                assert_eq!(message.payload_hash, "9e01c423ca440c5ec2beecc9d0a152b54fc8e7a416c931b7089eaf221605af17");
                assert_eq!(cost.amount, "0");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.common_meta.tx_id.as_deref(),
                    Some("RwUVL9in7fSCxZmVThP0eKM8Qvh3fJpVZQTPxU1mD8I=")
                );
            }
            _ => panic!("Expected MessageApproved event"),
        }
    }

    #[tokio::test]
    async fn test_handle_call_contract() {
        let transactions = fixture_transactions();
        let tx = &transactions[4];
        let ingestor = TONIngestor::new();

        let body = ingestor.body_if_call_contract(tx).unwrap();

        let res = ingestor
            .handle_call_contract(tx.clone(), &body)
            .await
            .unwrap();
        assert_eq!(res.len(), 2);
        let event = &res[0];

        match event {
            Event::Call {
                common, message, destination_chain, payload
            } => {
                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(payload, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gUmVsYXllciEAAAAAAAAAAAAAAAAA");
                assert_eq!(
                    message.message_id,
                    "0x06835ed473a483ee64f17186b98e6245cbb3f0dc24739af14fb36e33fbc33ff1"
                );
                assert_eq!(message.source_chain, "ton2");
                assert_eq!(message.payload_hash, "rqZSQ2cAD7SgqiCx1PY9qtHtnp33Fj8jCWc2EPLzfUs=");
                assert_eq!(message.source_address, "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("BoNe1HOkg+5k8XGGuY5iRcuz8Nwkc5rxT7NuM/vDP/E=")
                );
            }
            _ => panic!("Expected MessageApproved event"),
        }    }

}
