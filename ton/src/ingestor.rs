/*!

# TODO:
- Do not hardcode hash

*/

use router_api::CrossChainId;
use crate::boc_cc_message::TonCCMessage;
use crate::boc_nullified_message::NullifiedSuccessfullyMessage;
use crate::ton_op_codes::{OP_GATEWAY_EXECUTE, OP_MESSAGE_APPROVED, OP_NULLIFIED_SUCCESSFULLY};
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

pub struct TONIngestor<PC> {
    payload_cache: PC
}

impl<PC: PayloadCacheTrait> TONIngestor<PC> {
    pub fn new(payload_cache: PC) -> Self {
        Self {
            payload_cache,
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

        let cached = self.payload_cache.get(cc_id.clone()).await.map_err(|e| IngestorError::GenericError(e.to_string()));
        let cached_result = cached?;
        if cached_result.is_none() {
            return Err(IngestorError::GenericError(format!("Payload not found for CC ID: {:?}", cc_id)));
        }
        let payload_hash = cached_result.unwrap().message.payload_hash;
        
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
                payload_hash
            },
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        };

        self.payload_cache.clear(cc_id).await.unwrap_or(());
        
        Ok(vec![event])
    }
}

impl<PC: PayloadCacheTrait> IngestorTrait for TONIngestor<PC> {
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
    use relayer_base::gmp_api::gmp_types::{Event, GatewayV2Message, MessageExecutionStatus};
    use relayer_base::ton_types::{Transaction, TransactionsResponse};
    use std::fs;
    use relayer_base::payload_cache::{MockPayloadCacheTrait, PayloadCacheValue};

    fn fixture_transactions() -> Vec<Transaction> {
        let file_path = "tests/data/v3_transactions.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let transactions_response: TransactionsResponse =
            serde_json::from_str(&body).expect("Failed to deserialize test transaction data");

        transactions_response.transactions
    }

    #[tokio::test]
    async fn test_is_approved_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];
        let ingestor = TONIngestor::new(MockPayloadCacheTrait::new());
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
        let ingestor = TONIngestor::new(MockPayloadCacheTrait::new());

        let approved_body = ingestor.body_if_approved(tx);

        assert!(
            approved_body.is_none(),
            "Expected the transaction not to be an approved message"
        );
    }

    #[tokio::test]
    async fn test_is_executed_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];
        let ingestor = TONIngestor::new(MockPayloadCacheTrait::new());

        let approved_body = ingestor.body_if_executed(tx);

        assert_eq!(approved_body.unwrap(), "te6cckECCAEAAV0ABIsAAAAFgBIHqwhg5lg4ES2+GWhwn4EVgGvmj7MoTr6OJXwhB8Byr9KMj8CFtEqwFmUtJVgVpEqk3ftJTCRWAx2zya/xlWvwAQIDBACIMHhmMmI3NDFmYjBiMmMyZmNmOTJhY2E4MjM5NWJjNjVkYWI0ZGQ4MjM5YTEyZjM2NmQ2MDQ1NzU1ZTBiMDJjMmEyLTEAHGF2YWxhbmNoZS1mdWppAFQweGQ3MDY3QWUzQzM1OWU4Mzc4OTBiMjhCN0JEMGQyMDg0Q2ZEZjQ5YjUDAAUGBwDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD0hlbGxvIGZyb20gdG9uIQAAAAAAAAAAAAAAAAAAAAAAAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMtlN//0=");
    }

    #[tokio::test]
    async fn test_is_not_executed_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];
        let ingestor = TONIngestor::new(MockPayloadCacheTrait::new());
        let approved_body = ingestor.body_if_executed(tx);

        assert!(
            approved_body.is_none(),
            "Expected transaction not to be execute message"
        );
    }

    #[tokio::test]
    async fn test_handle_executed() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];
        let payload_cache = MockPayloadCacheTrait::new();
        let ingestor = TONIngestor::new(payload_cache);
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
        let mut payload_cache = MockPayloadCacheTrait::new();
        payload_cache
            .expect_get()
            .returning(|_| Box::pin(async { Ok(Some(PayloadCacheValue {
                message: GatewayV2Message {
                    message_id: "aaa".to_string(),
                    source_chain: "bbb".to_string(),
                    source_address: "ccc".to_string(),
                    destination_address: "ddd".to_string(),
                    payload_hash: "eee".to_string(),
                },
                payload: "fff".to_string(),
            })) }))
            .times(1);

        payload_cache
            .expect_clear()
            .returning(|_| Box::pin(async { Ok(()) }))
            .times(1);

        let ingestor = TONIngestor::new(payload_cache);

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
                assert_eq!(message.payload_hash, "eee");
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
}
