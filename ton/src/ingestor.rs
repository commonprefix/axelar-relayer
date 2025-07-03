use std::marker::PhantomData;
use relayer_base::database::Database;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, ConstructProofTask, Event, EventMetadata, ExecuteTaskFields, GatewayV2Message, MessageApprovedEventMetadata, MessageExecutedEventMetadata, MessageExecutionStatus, ReactToWasmEventTask, RetryTask, VerifyTask};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::subscriber::ChainTransaction;
use relayer_base::ton_types::Transaction;
use crate::extract_log::TonLog;

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

}

impl<DB: Database> IngestorTrait for TONIngestor<DB> {
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        todo!()
    }
    
    async fn handle_transaction(&self, tx: ChainTransaction) -> Result<Vec<Event>, IngestorError> {

        let ChainTransaction::TON(tx) = tx else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!("{:?}", tx)))
        };

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
                    payload_hash: "aea6524367000fb4a0aa20b1d4f63daad1ed9e9df7163f2309673610f2f37d4b".to_string(),
                },
                cost: Amount { token_id: None, amount: "0".to_string() },
            };
            return Ok(vec![event]);
        }

        Ok(vec![])
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        todo!()
    }

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        todo!()
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use relayer_base::ton_types::{Transaction, TransactionsResponse};

    #[tokio::test]
    async fn test_is_approved_message() {
        todo!("Functionality will soon change")
    }

    fn fixture_transactions() -> Vec<Transaction> {
        let file_path = "tests/data/v3_transactions.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let transactions_response: TransactionsResponse = serde_json::from_str(&body)
            .expect("Failed to deserialize test transaction data");

        transactions_response.transactions
    }

    async fn test_handle_approved_message() {
        let transactions = fixture_transactions();
        todo!("Functionality will soon change")
    }
}