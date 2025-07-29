use std::sync::Arc;

use tracing::{debug, error, warn};
use xrpl_api::{
    ResultCategory, SubmitRequest, SubmitResponse, Transaction, TransactionResult, TxRequest,
};

use relayer_base::{
    error::BroadcasterError,
    includer::{BroadcastResult, Broadcaster},
    utils::extract_hex_xrpl_memo,
};

use crate::models::queued_transactions::QueuedTransactionsModel;

use super::client::XRPLClientTrait;

pub struct XRPLBroadcaster<QM: QueuedTransactionsModel, X: XRPLClientTrait> {
    client: Arc<X>,
    queued_tx_model: QM,
}

impl<QM: QueuedTransactionsModel, X: XRPLClientTrait> XRPLBroadcaster<QM, X> {
    pub fn new(client: Arc<X>, queued_tx_model: QM) -> error_stack::Result<Self, BroadcasterError> {
        Ok(XRPLBroadcaster {
            client,
            queued_tx_model,
        })
    }

    pub async fn handle_queued_tx(
        &self,
        tx: &Transaction,
        tx_hash: &str,
    ) -> Result<(), BroadcasterError> {
        self.queued_tx_model
            .store_queued_transaction(
                tx_hash,
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

impl<QM: QueuedTransactionsModel, X: XRPLClientTrait> Broadcaster for XRPLBroadcaster<QM, X> {
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
                extract_hex_xrpl_memo(memos, "source_chain")
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

            if let Err(e) = self.handle_queued_tx(&tx, &tx_hash).await {
                error!("Failed to store queued transaction: {:?}", e);
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
        } else if response.engine_result == TransactionResult::terQUEUED {
            debug!("Refund transaction {} is queued (terQUEUED)", tx_hash);

            if let Err(e) = self.handle_queued_tx(&tx, tx_hash).await {
                error!("Failed to store queued refund transaction: {:?}", e);
            } else {
                debug!("Successfully stored queued refund transaction");
            }

            Ok(tx_hash.clone())
        } else {
            Err(BroadcasterError::RPCCallFailed(format!(
                "Refund transaction failed: {:?}: {}",
                response.engine_result, response.engine_result_message
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::models::queued_transactions::MockQueuedTransactionsModel;
    use crate::{broadcaster::XRPLBroadcaster, client::MockXRPLClientTrait};
    use relayer_base::error::BroadcasterError;
    use relayer_base::includer::{BroadcastResult, Broadcaster};
    use serde_json;
    use std::future;
    use std::sync::Arc;
    use xrpl_api::{SubmitRequest, Transaction};
    use xrpl_api::{SubmitResponse, TransactionResult};

    // Helper function to return responses because xrpl_api does not support default.
    // Modify accordingly for each call

    fn setup_broadcast_prover_tests(
        blob: &str,
        result: TransactionResult,
        account: &str,
        sequence: i64,
        tx_hash: &str,
        transaction_type: &str,
    ) -> (
        MockQueuedTransactionsModel,
        MockXRPLClientTrait,
        SubmitResponse,
        Transaction,
    ) {
        let tx_json = format!(
            r#"{{"TransactionType":"{}","Account":"{}","Destination":"{}","Amount":"1000","Sequence":{},"Fee":"12","Flags":2147483648,"Memos":[{{"Memo":{{"MemoType":"6d6573736167655f6964","MemoData":"6d6573736167655f6964","MemoFormat":"hex"}}}},{{"Memo":{{"MemoType":"736f757263655f636861696e","MemoData":"736f757263655f636861696e","MemoFormat":"hex"}}}}],"hash":"{}"}}"#,
            transaction_type, account, account, sequence, tx_hash
        );
        let tx: Transaction = serde_json::from_str(&tx_json).unwrap();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();
        let mock_client = MockXRPLClientTrait::new();
        let fake_response = client_response(&tx, blob, result);
        (mock_queued_tx_model, mock_client, fake_response, tx)
    }

    fn client_response(tx: &Transaction, blob: &str, result: TransactionResult) -> SubmitResponse {
        let engine_result_message = match result {
            TransactionResult::terNO_AUTH => "Transaction not authorized".to_string(),
            _ => String::new(),
        };

        SubmitResponse {
            engine_result: result,
            engine_result_code: 0,
            engine_result_message,
            tx_blob: blob.to_string(),
            tx_json: tx.clone(),
            accepted: false,
            account_sequence_available: 0,
            account_sequence_next: 0,
            applied: false,
            broadcast: false,
            kept: false,
            queued: result == TransactionResult::terQUEUED,
            open_ledger_cost: String::new(),
            validated_ledger_index: 0,
        }
    }

    #[tokio::test]
    async fn test_handle_queued_tx() {
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence: i64 = 123;
        let blob = "blob";
        let transaction_type = "Payment";

        let (mut mock_queued_tx_model, mock_client, _fake_response, tx) =
            setup_broadcast_prover_tests(
                blob,
                TransactionResult::terQUEUED,
                account,
                sequence,
                tx_hash,
                transaction_type,
            );

        mock_queued_tx_model
            .expect_store_queued_transaction()
            .withf(move |h, a, s| h == tx_hash && a == account && *s == sequence)
            .times(1)
            .returning(|_, _, _| Box::pin(async { Ok(()) }));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(mock_client),
            queued_tx_model: mock_queued_tx_model,
        };

        let result = broadcaster.handle_queued_tx(&tx, tx_hash).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_broadcast_prover_message_ter_queued() {
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence: i64 = 123;
        let blob = "blob";
        let transaction_type = "Payment";
        let (mut mock_queued_tx_model, mut mock_client, fake_response, tx) =
            setup_broadcast_prover_tests(
                blob,
                TransactionResult::terQUEUED,
                account,
                sequence,
                tx_hash,
                transaction_type,
            );

        mock_client
            .expect_call::<SubmitRequest>()
            .times(1)
            .returning(move |_| Box::pin(future::ready(Ok(fake_response.clone()))));

        // TODO : This is not really the check we want to make.
        // We need to check that handle_queued_tx is called instead
        mock_queued_tx_model
            .expect_store_queued_transaction()
            .withf(move |h, a, s| h == tx_hash && a == account && *s == sequence)
            .times(1)
            .returning(|_, _, _| Box::pin(async { Ok(()) }));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(mock_client),
            queued_tx_model: mock_queued_tx_model,
        };

        let broadcast_result = broadcaster
            .broadcast_prover_message(blob.to_string())
            .await
            .unwrap();

        let expected_broadcast_result = BroadcastResult {
            transaction: tx,
            tx_hash: tx_hash.to_string(),
            status: Ok(()),
            message_id: Some("message_id".to_string()),
            source_chain: Some("source_chain".to_string()),
        };

        assert_eq!(broadcast_result, expected_broadcast_result);
    }

    #[tokio::test]
    async fn test_broadcast_prover_message_ticket_create_tef_past_seq() {
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence: i64 = 123;
        let blob = "blob";
        let transaction_type = "TicketCreate";
        let (mock_queued_tx_model, mut mock_client, fake_response, tx) =
            setup_broadcast_prover_tests(
                blob,
                TransactionResult::tefPAST_SEQ,
                account,
                sequence,
                tx_hash,
                transaction_type,
            );

        mock_client
            .expect_call::<SubmitRequest>()
            .times(1)
            .returning(move |_| Box::pin(future::ready(Ok(fake_response.clone()))));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(mock_client),
            queued_tx_model: mock_queued_tx_model,
        };

        let broadcast_result = broadcaster
            .broadcast_prover_message(blob.to_string())
            .await
            .unwrap();

        let expected_broadcast_result = BroadcastResult {
            transaction: tx,
            tx_hash: tx_hash.to_string(),
            status: Ok(()),
            message_id: None,
            source_chain: None,
        };

        assert_eq!(broadcast_result, expected_broadcast_result);
    }

    // all tes should return OK
    #[tokio::test]
    async fn test_broadcast_prover_message_tes_success() {
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence: i64 = 123;
        let blob = "blob";
        let transaction_type = "Payment";
        let (mock_queued_tx_model, mut mock_client, fake_response, tx) =
            setup_broadcast_prover_tests(
                blob,
                TransactionResult::tesSUCCESS,
                account,
                sequence,
                tx_hash,
                transaction_type,
            );

        mock_client
            .expect_call::<SubmitRequest>()
            .times(1)
            .returning(move |_| Box::pin(future::ready(Ok(fake_response.clone()))));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(mock_client),
            queued_tx_model: mock_queued_tx_model,
        };

        let broadcast_result = broadcaster
            .broadcast_prover_message(blob.to_string())
            .await
            .unwrap();

        let expected_broadcast_result = BroadcastResult {
            transaction: tx,
            tx_hash: tx_hash.to_string(),
            status: Ok(()),
            message_id: Some("message_id".to_string()),
            source_chain: Some("source_chain".to_string()),
        };

        assert_eq!(broadcast_result, expected_broadcast_result);
    }

    // all tec should return OK
    #[tokio::test]
    async fn test_broadcast_prover_message_tec_internal() {
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence: i64 = 123;
        let blob = "blob";
        let transaction_type = "Payment";
        let (mock_queued_tx_model, mut mock_client, fake_response, tx) =
            setup_broadcast_prover_tests(
                blob,
                TransactionResult::tecINTERNAL,
                account,
                sequence,
                tx_hash,
                transaction_type,
            );

        mock_client
            .expect_call::<SubmitRequest>()
            .times(1)
            .returning(move |_| Box::pin(future::ready(Ok(fake_response.clone()))));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(mock_client),
            queued_tx_model: mock_queued_tx_model,
        };

        let broadcast_result = broadcaster
            .broadcast_prover_message(blob.to_string())
            .await
            .unwrap();

        let expected_broadcast_result = BroadcastResult {
            transaction: tx,
            tx_hash: tx_hash.to_string(),
            status: Ok(()),
            message_id: Some("message_id".to_string()),
            source_chain: Some("source_chain".to_string()),
        };

        assert_eq!(broadcast_result, expected_broadcast_result);
    }

    // other ter cases (other than terQUEUED) should return error
    #[tokio::test]
    async fn test_broadcast_prover_message_ter_no_auth() {
        let tx_hash = "DUMMY_HASH";
        let account = "rDummyAccount";
        let sequence: i64 = 123;
        let blob = "blob";
        let transaction_type = "Payment";
        let (mock_queued_tx_model, mut mock_client, fake_response, tx) =
            setup_broadcast_prover_tests(
                blob,
                TransactionResult::terNO_AUTH,
                account,
                sequence,
                tx_hash,
                transaction_type,
            );

        mock_client
            .expect_call::<SubmitRequest>()
            .times(1)
            .returning(move |_| Box::pin(future::ready(Ok(fake_response.clone()))));

        let broadcaster = XRPLBroadcaster {
            client: Arc::new(mock_client),
            queued_tx_model: mock_queued_tx_model,
        };

        let result = broadcaster.broadcast_prover_message(blob.to_string()).await;
        assert!(result.is_ok());

        let broadcast_result = result.unwrap();
        let expected_broadcast_result = BroadcastResult {
            transaction: tx,
            tx_hash: tx_hash.to_string(),
            status: Err(BroadcasterError::RPCCallFailed(format!(
                "Transaction failed: {:?}: {}",
                TransactionResult::terNO_AUTH,
                "Transaction not authorized"
            ))),
            message_id: Some("message_id".to_string()),
            source_chain: Some("source_chain".to_string()),
        };

        assert_eq!(broadcast_result, expected_broadcast_result);
    }
}
