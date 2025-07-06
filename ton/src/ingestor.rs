/*!

# TODO: Do not hardcode hash

*/

use std::marker::PhantomData;
use relayer_base::database::Database;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, ConstructProofTask, Event, EventMetadata, GatewayV2Message, MessageApprovedEventMetadata, MessageExecutedEventMetadata, MessageExecutionStatus, ReactToWasmEventTask, RetryTask, VerifyTask};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::subscriber::ChainTransaction;
use relayer_base::ton_types::Transaction;
use crate::extract_log::TonLog;
use crate::nullified_message::NullifiedSuccessfullyMessage;

pub struct TONIngestor<DB> {
    _marker: PhantomData<DB>,

}

impl<DB: Database> TONIngestor<DB> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
    fn body_if_approved(tx: &Transaction) -> Option<String> {
        let has_out_empty_dest = tx.out_msgs.iter().any(|msg| {
            msg.destination.as_deref().unwrap_or("").is_empty()
        });

        let in_msg_opcode_d = tx.in_msg.as_ref().map_or(false, |msg| {
            matches!(msg.opcode.as_deref(), Some("0x0000000d"))
        });

        if has_out_empty_dest && in_msg_opcode_d {
            tx.out_msgs.iter()
                .find(|msg| msg.destination.as_deref().unwrap_or("").is_empty())
                .map(|msg| msg.message_content.body.clone())
        } else {
            None
        }
    }

    fn body_if_executed(tx: &Transaction) -> Option<String> {
        let has_out_empty_dest = tx.out_msgs.iter().any(|msg| {
            msg.destination.as_deref().unwrap_or("").is_empty()
        });

        let out_msg_opcode = tx.out_msgs.iter().any(|msg| {
            matches!(msg.opcode.as_deref(), Some("0x0000000c"))
        });

        if has_out_empty_dest && out_msg_opcode {
            tx.in_msg.iter()
                .find(|msg| msg.opcode.as_deref().unwrap_or("") == "0x00000005")
                .map(|msg| msg.message_content.body.clone())
        } else {
            None
        }
    }
}

impl<DB: Database> IngestorTrait for TONIngestor<DB> {
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        println!("handle_verify: {:?}", task);

        Err(IngestorError::GenericError("Still not implemented".to_string()))
    }
    
    async fn handle_transaction(&self, tx: ChainTransaction) -> Result<Vec<Event>, IngestorError> {

        let ChainTransaction::TON(tx) = tx else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!("{:?}", tx)))
        };

        if let Some(body) = Self::body_if_executed(&tx) {
            let message = NullifiedSuccessfullyMessage::from_boc_b64(&body).map_err(|e| IngestorError::GenericError(e.to_string()))?;
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
                cost: Amount { token_id: None, amount: "0".to_string() },
            };
            return Ok(vec![event]);
        }

        if let Some(body) = Self::body_if_approved(&tx) {
            let log = TonLog::from_boc_b64(&body).unwrap();
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
                        }, command_id: None }
                    ),
                },
                message: GatewayV2Message {
                    message_id: log.message_id,
                    source_chain: log.source_chain,
                    source_address: log.source_address,
                    destination_address: log.destination_address,
                    payload_hash: "9e01c423ca440c5ec2beecc9d0a152b54fc8e7a416c931b7089eaf221605af17".to_string(),
                },
                cost: Amount { token_id: None, amount: "0".to_string() },
            };
            return Ok(vec![event]);
        }

        Ok(vec![])
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        println!("handle_wasm_event: {:?}", task);

        Err(IngestorError::GenericError("Still not implemented".to_string()))
    }

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        println!("handle_construct_proof: {:?}", task);

        Err(IngestorError::GenericError("Still not implemented".to_string()))
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        println!("handle_retriable_task: {:?}", task);

        Err(IngestorError::GenericError("Still not implemented".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use relayer_base::database::{Database, PostgresDB};
    use relayer_base::ton_types::{Transaction, TransactionsResponse};

    fn fixture_transactions() -> Vec<Transaction> {
        let file_path = "tests/data/v3_transactions.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let transactions_response: TransactionsResponse = serde_json::from_str(&body)
            .expect("Failed to deserialize test transaction data");

        transactions_response.transactions
    }

    #[tokio::test]
    async fn test_is_approved_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];

        let approved_body = super::TONIngestor::<PostgresDB>::body_if_approved(tx);

        assert!(
            approved_body.is_some(),
            "Expected transaction to be an approved message"
        );
    }

    #[tokio::test]
    async fn test_is_not_approved_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];

        let approved_body = super::TONIngestor::<PostgresDB>::body_if_approved(tx);

        assert!(
            approved_body.is_none(),
            "Expected the transaction not to be an approved message"
        );
    }

    #[tokio::test]
    async fn test_is_executed_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[3];

        let approved_body = super::TONIngestor::<PostgresDB>::body_if_executed(tx);

        assert_eq!(approved_body.unwrap(), "te6cckECCAEAAV0ABIsAAAAFgBIHqwhg5lg4ES2+GWhwn4EVgGvmj7MoTr6OJXwhB8Byr9KMj8CFtEqwFmUtJVgVpEqk3ftJTCRWAx2zya/xlWvwAQIDBACIMHhmMmI3NDFmYjBiMmMyZmNmOTJhY2E4MjM5NWJjNjVkYWI0ZGQ4MjM5YTEyZjM2NmQ2MDQ1NzU1ZTBiMDJjMmEyLTEAHGF2YWxhbmNoZS1mdWppAFQweGQ3MDY3QWUzQzM1OWU4Mzc4OTBiMjhCN0JEMGQyMDg0Q2ZEZjQ5YjUDAAUGBwDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD0hlbGxvIGZyb20gdG9uIQAAAAAAAAAAAAAAAAAAAAAAAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMtlN//0=");
    }


    #[tokio::test]
    async fn test_is_not_executed_message() {
        let transactions = fixture_transactions();
        let tx = &transactions[0];

        let approved_body = super::TONIngestor::<PostgresDB>::body_if_executed(tx);

        assert!(
            approved_body.is_none(),
            "Expected transaction not to be execute message"
        );
    }
}