use std::sync::Arc;

use tracing::{debug, error, warn};
use xrpl_api::{
    ResultCategory, SubmitRequest, SubmitResponse, Transaction, TransactionResult, TxRequest,
};

use relayer_base::{
    database::Database,
    error::BroadcasterError,
    includer::{BroadcastResult, Broadcaster},
    utils::extract_hex_xrpl_memo,
};

use super::client::XRPLClient;

pub struct XRPLBroadcaster<DB: Database> {
    client: Arc<XRPLClient>,
    db: DB,
}

impl<DB: Database> XRPLBroadcaster<DB> {
    pub fn new(client: Arc<XRPLClient>, db: DB) -> error_stack::Result<Self, BroadcasterError> {
        Ok(XRPLBroadcaster { client, db })
    }

    pub async fn handle_queued_tx(
        &self,
        tx: &Transaction,
        tx_hash: &str,
    ) -> Result<(), BroadcasterError> {
        self.db
            .store_queued_transaction(
                &tx_hash,
                &tx.common().account.to_string(),
                tx.common().sequence as i64,
            )
            .await
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))
    }
}

fn log_and_return_error(
    tx: &Transaction,
    response: &SubmitResponse,
    message_id: Option<String>,
    source_chain: Option<String>,
) -> Result<BroadcastResult<Transaction>, BroadcasterError> {
    error!(
        "Transaction failed: {:?}: {}",
        response.engine_result.clone(),
        response.engine_result_message.clone()
    );
    Ok(BroadcastResult {
        transaction: tx.clone(),
        tx_hash: tx.common().hash.to_owned().ok_or_else(|| {
            BroadcasterError::GenericError("Transaction hash not found".to_string())
        })?,
        status: Err(BroadcasterError::RPCCallFailed(format!(
            "Transaction failed: {:?}: {}",
            response.engine_result, response.engine_result_message
        ))),
        message_id,
        source_chain,
    })
}

impl<DB: Database> Broadcaster for XRPLBroadcaster<DB> {
    type Transaction = Transaction;

    async fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        let req = SubmitRequest::new(tx_blob);
        let response = self
            .client
            .call(req)
            .await
            .map_err(|e| BroadcasterError::RPCCallFailed(e.to_string()))?;

        let mut message_id = None;
        let mut source_chain = None;
        let tx = response.tx_json.clone();
        if let xrpl_api::Transaction::Payment(payment_transaction) = &tx {
            let memos = payment_transaction.common.memos.clone();
            message_id = Some(
                extract_hex_xrpl_memo(memos.clone(), "message_id")
                    .map_err(|e| BroadcasterError::GenericError(e.to_string()))?,
            );
            source_chain = Some(
                extract_hex_xrpl_memo(memos.clone(), "source_chain")
                    .map_err(|e| BroadcasterError::GenericError(e.to_string()))?,
            );
        }

        let tx_hash = tx.common().hash.to_owned().ok_or_else(|| {
            BroadcasterError::RPCCallFailed("Transaction hash not found".to_string())
        })?;
        let response_category = response.engine_result.category();
        debug!("Response category: {:?}", response_category);
        if response_category == ResultCategory::Tec || response_category == ResultCategory::Tes {
            Ok(BroadcastResult {
                transaction: tx.clone(),
                tx_hash,
                status: Ok(()),
                message_id,
                source_chain,
            })
        } else if response_category == ResultCategory::Tef {
            if matches!(tx, Transaction::TicketCreate(_))
                && response.engine_result == TransactionResult::tefPAST_SEQ
            {
                // Note: This is expected to happen, as there might be a race condition between different
                // ticket create signing sessions, where the same sequence number was used.
                return Ok(BroadcastResult {
                    transaction: tx.clone(),
                    tx_hash,
                    status: Ok(()),
                    message_id,
                    source_chain,
                });
            }
            let req = TxRequest::new(&tx_hash);
            match self.client.call(req).await {
                Ok(query_response) => match query_response.tx.common().validated {
                    Some(true) => {
                        warn!("Transaction already submitted: {:?}", tx_hash);
                        Ok(BroadcastResult {
                            transaction: tx.clone(),
                            tx_hash,
                            status: Ok(()),
                            message_id,
                            source_chain,
                        })
                    }
                    _ => log_and_return_error(&tx, &response, message_id, source_chain),
                },
                Err(_) => log_and_return_error(&tx, &response, message_id, source_chain),
            }
        } else if response.engine_result == TransactionResult::terQUEUED {
            debug!("Transaction {} is queued (terQUEUED)", tx_hash);

            let maybe_stored_transaction = self.handle_queued_tx(&tx, &tx_hash).await;

            if maybe_stored_transaction.is_err() {
                error!(
                    "Failed to store queued transaction: {:?}",
                    maybe_stored_transaction
                );
            } else {
                debug!("Successfully stored queued transaction");
            }

            return Ok(BroadcastResult {
                transaction: tx.clone(),
                tx_hash,
                status: Ok(()),
                message_id,
                source_chain,
            });
        } else {
            log_and_return_error(&tx, &response, message_id, source_chain)
        }
    }

    async fn broadcast_refund(&self, tx_blob: String) -> Result<String, BroadcasterError> {
        let req = SubmitRequest::new(tx_blob);
        let response = self
            .client
            .call(req)
            .await
            .map_err(|e| BroadcasterError::RPCCallFailed(e.to_string()))?;

        let tx = response.tx_json.clone();
        let tx_hash = tx.common().hash.as_ref().ok_or_else(|| {
            BroadcasterError::RPCCallFailed("Transaction hash not found".to_string())
        })?;
        if response.engine_result == TransactionResult::tesSUCCESS {
            Ok(tx_hash.clone())
        } else {
            Err(BroadcasterError::RPCCallFailed(format!(
                "Transaction failed: {:?}: {}",
                response.engine_result, response.engine_result_message
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relayer_base::database::MockDatabase;
    use std::sync::Arc;
    use xrpl_api::Transaction;

    #[tokio::test]
    async fn test_handle_queued_tx() {
        let mut mock_db = MockDatabase::new();
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence = 123;

        mock_db
            .expect_store_queued_transaction()
            .withf(move |h, a, s| h == tx_hash && a == account && *s == sequence)
            .times(1)
            .returning(|_, _, _| Box::pin(async { Ok(()) }));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(XRPLClient::new("http://dummy.url", 0).unwrap()),
            db: mock_db,
        };

        let tx_json = format!(
            r#"{{
                "TransactionType": "Payment",
                "Account": "{}",
                "Destination": "{}",
                "Amount": "1000",
                "Sequence": {},
                "Fee": "12",
                "Flags": 2147483648
            }}"#,
            account, account, sequence
        );
        let tx: Transaction = serde_json::from_str(&tx_json).unwrap();

        let result = broadcaster.handle_queued_tx(&tx, tx_hash).await;

        assert!(result.is_ok());
    }
}
