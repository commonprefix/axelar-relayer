use crate::{
    client::XRPLClientTrait,
    models::queued_transactions::{QueuedTransactionStatus, QueuedTransactionsModel},
};

use crate::error::QueuedTxMonitorError;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use xrpl_api::TxRequest;
use xrpl_http_client;

const MAX_RETRIES: i32 = 20;

pub enum TxStatus {
    Confirmed,
    Queued,
    Dropped,
}

pub struct XrplQueuedTxMonitor<QM: QueuedTransactionsModel, X: XRPLClientTrait> {
    client: Arc<X>,
    queued_tx_model: QM,
}

impl<QM: QueuedTransactionsModel, X: XRPLClientTrait> XrplQueuedTxMonitor<QM, X> {
    pub fn new(client: Arc<X>, queued_tx_model: QM) -> Self {
        Self {
            client,
            queued_tx_model,
        }
    }

    async fn work(&self) {
        if let Err(e) = self.check_queued_transactions().await {
            error!("Error checking queued transactions: {}", e);
        }
    }

    pub async fn run(&self) {
        loop {
            info!("XrplQueuedTxMonitor is alive.");
            self.work().await;
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    async fn check_queued_transactions(&self) -> Result<(), QueuedTxMonitorError> {
        let queued_txs = self
            .queued_tx_model
            .get_queued_transactions_ready_for_check()
            .await
            .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;

        debug!("Found {} queued transactions to check", queued_txs.len());

        for tx in queued_txs {
            if tx.retries >= MAX_RETRIES {
                warn!(
                    "Transaction {} exceeded max retries, marking as expired",
                    tx.tx_hash
                );
                self.queued_tx_model
                    .update_transaction_status(&tx.tx_hash, QueuedTransactionStatus::Expired)
                    .await
                    .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                continue;
            }

            match self.check_transaction_status(&tx.tx_hash).await {
                Ok(TxStatus::Confirmed) => {
                    debug!("Transaction {} confirmed", tx.tx_hash);
                    self.queued_tx_model
                        .update_transaction_status(&tx.tx_hash, QueuedTransactionStatus::Confirmed)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
                Ok(TxStatus::Queued) => {
                    debug!(
                        "Transaction {} still queued, incrementing retry count to {}",
                        tx.tx_hash,
                        tx.retries + 1
                    );
                    self.queued_tx_model
                        .increment_retry(&tx.tx_hash)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
                Ok(TxStatus::Dropped) => {
                    warn!("Transaction {} dropped", tx.tx_hash);
                    self.queued_tx_model
                        .update_transaction_status(&tx.tx_hash, QueuedTransactionStatus::Dropped)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
                Err(e) => {
                    error!("Error checking transaction {}: {}", tx.tx_hash, e);
                    self.queued_tx_model
                        .increment_retry(&tx.tx_hash)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
            }
        }

        Ok(())
    }

    pub async fn check_transaction_status(
        &self,
        tx_hash: &str,
    ) -> Result<TxStatus, QueuedTxMonitorError> {
        let req = TxRequest::new(tx_hash);

        match self.client.call(req).await {
            Ok(query_response) => {
                if query_response.tx.common().validated == Some(true) {
                    debug!("Transaction {} confirmed", tx_hash);
                    Ok(TxStatus::Confirmed)
                } else {
                    debug!("Transaction {} queued", tx_hash);
                    Ok(TxStatus::Queued)
                }
            }
            Err(e) => match e {
                xrpl_http_client::error::Error::Api(error_code) if error_code == "txnNotFound" => {
                    debug!("Transaction {} not found, marking as dropped", tx_hash);
                    Ok(TxStatus::Dropped)
                }
                _ => {
                    // TODO: How to handle this?
                    debug!("Error checking transaction {}: {}", tx_hash, e);
                    Err(QueuedTxMonitorError::XRPLClientError(e.to_string()))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::models::queued_transactions::QueuedTransactionStatus;
    use crate::queued_tx_monitor::QueuedTxMonitorError;
    use crate::{
        client::MockXRPLClientTrait,
        models::queued_transactions::{MockQueuedTransactionsModel, QueuedTransaction},
        queued_tx_monitor::{TxStatus, MAX_RETRIES},
        XrplQueuedTxMonitor,
    };
    use mockall::predicate::eq;
    use xrpl_api::{PaymentTransaction, Transaction, TransactionCommon, TxRequest, TxResponse};

    #[tokio::test]
    async fn test_check_transaction_status_confirmed() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(move |_| {
                Ok(TxResponse {
                    tx: Transaction::Payment(PaymentTransaction {
                        common: TransactionCommon {
                            validated: Some(true),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                })
            });

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_ok());
        let tx_status = result.unwrap();
        assert!(matches!(tx_status, TxStatus::Confirmed));
    }

    #[tokio::test]
    async fn test_check_transaction_status_queued() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(move |_| {
                Ok(TxResponse {
                    tx: Transaction::Payment(PaymentTransaction {
                        common: TransactionCommon {
                            validated: Some(false),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                })
            });

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_ok());
        let tx_status = result.unwrap();
        assert!(matches!(tx_status, TxStatus::Queued));
    }

    #[tokio::test]
    async fn test_check_transaction_status_dropped() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(move |_| {
                Err(xrpl_http_client::error::Error::Api(
                    "txnNotFound".to_string(),
                ))
            });

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_ok());
        let tx_status = result.unwrap();
        assert!(matches!(tx_status, TxStatus::Dropped));
    }

    #[tokio::test]
    async fn test_check_transaction_status_error() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(move |_| {
                Err(xrpl_http_client::error::Error::Api(
                    "test error".to_string(),
                ))
            });

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_check_transaction_status_validated_none() {
        let mut mock_client = MockXRPLClientTrait::default();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(|_| {
                Ok(TxResponse {
                    tx: Transaction::Payment(PaymentTransaction {
                        common: TransactionCommon {
                            validated: None, // This is currently treated as queued (should it?)
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                })
            });

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), TxStatus::Queued));
    }

    #[tokio::test]
    async fn test_check_transaction_status_network_error() {
        let mut mock_client = MockXRPLClientTrait::default();
        let mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(|_| {
                Err(xrpl_http_client::error::Error::Internal(
                    "Connection timeout".to_string(),
                ))
            });

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.err(),
            Some(QueuedTxMonitorError::XRPLClientError(_))
        ));
    }

    #[tokio::test]
    async fn test_check_queued_transactions() {
        let mut mock_client = MockXRPLClientTrait::default();
        let mut mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_queued_tx_model
            .expect_get_queued_transactions_ready_for_check()
            .times(1)
            .returning(|| {
                Ok(vec![
                    QueuedTransaction {
                        tx_hash: "DUMMY_HASH".to_string(),
                        retries: 0,
                        account: Some("DUMMY_ACCOUNT".to_string()),
                        sequence: Some(1),
                        status: QueuedTransactionStatus::Queued,
                    },
                    QueuedTransaction {
                        tx_hash: "DUMMY_HASH2".to_string(),
                        retries: 0,
                        account: Some("DUMMY_ACCOUNT".to_string()),
                        sequence: Some(1),
                        status: QueuedTransactionStatus::Queued,
                    },
                    // Expired
                    QueuedTransaction {
                        tx_hash: "DUMMY_HASH3".to_string(),
                        retries: MAX_RETRIES,
                        account: Some("DUMMY_ACCOUNT".to_string()),
                        sequence: Some(1),
                        status: QueuedTransactionStatus::Queued,
                    },
                    QueuedTransaction {
                        tx_hash: "DUMMY_HASH4".to_string(),
                        retries: 0,
                        account: Some("DUMMY_ACCOUNT".to_string()),
                        sequence: Some(1),
                        status: QueuedTransactionStatus::Queued,
                    },
                ])
            });

        // Confirmed
        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .return_once(|_| {
                Ok(TxResponse {
                    tx: Transaction::Payment(PaymentTransaction {
                        common: TransactionCommon {
                            validated: Some(true),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                })
            });

        // Dropped
        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .return_once(|_| {
                Err(xrpl_http_client::error::Error::Api(
                    "txnNotFound".to_string(),
                ))
            });

        // Queued
        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .return_once(|_| {
                Ok(TxResponse {
                    tx: Transaction::Payment(PaymentTransaction {
                        common: TransactionCommon {
                            validated: Some(false),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                })
            });

        mock_queued_tx_model
            .expect_update_transaction_status()
            .with(eq("DUMMY_HASH3"), eq(QueuedTransactionStatus::Expired))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_queued_tx_model
            .expect_update_transaction_status()
            .with(eq("DUMMY_HASH"), eq(QueuedTransactionStatus::Confirmed))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_queued_tx_model
            .expect_update_transaction_status()
            .with(eq("DUMMY_HASH2"), eq(QueuedTransactionStatus::Dropped))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_queued_tx_model
            .expect_increment_retry()
            .with(eq("DUMMY_HASH4"))
            .times(1)
            .returning(|_| Ok(()));

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);

        let result = queued_tx_monitor.check_queued_transactions().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_queued_transactions_db_get_error() {
        let mock_client = MockXRPLClientTrait::default();
        let mut mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_queued_tx_model
            .expect_get_queued_transactions_ready_for_check()
            .times(1)
            .returning(|| Err(anyhow::anyhow!("Database connection failed")));

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor.check_queued_transactions().await;

        assert!(result.is_err());
        assert!(matches!(
            result.err(),
            Some(QueuedTxMonitorError::DatabaseError(_))
        ));
    }

    #[tokio::test]
    async fn test_check_queued_transactions_empty_queue() {
        let mock_client = MockXRPLClientTrait::default();
        let mut mock_queued_tx_model = MockQueuedTransactionsModel::new();

        mock_queued_tx_model
            .expect_get_queued_transactions_ready_for_check()
            .times(1)
            .returning(|| Ok(vec![]));

        let queued_tx_monitor =
            XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_queued_tx_model);
        let result = queued_tx_monitor.check_queued_transactions().await;

        assert!(result.is_ok());
    }
}
