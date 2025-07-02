use crate::client::XRPLClientTrait;

use relayer_base::{database::Database, error::QueuedTxMonitorError};
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

pub struct XrplQueuedTxMonitor<DB: Database, X: XRPLClientTrait> {
    client: Arc<X>,
    db: DB,
}

impl<DB: Database, X: XRPLClientTrait> XrplQueuedTxMonitor<DB, X> {
    pub fn new(client: Arc<X>, db: DB) -> Self {
        Self { client, db }
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
            .db
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
                self.db
                    .mark_queued_transaction_expired(&tx.tx_hash)
                    .await
                    .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                continue;
            }

            match self.check_transaction_status(&tx.tx_hash).await {
                Ok(TxStatus::Confirmed) => {
                    info!("Transaction {} confirmed", tx.tx_hash);
                    self.db
                        .mark_queued_transaction_confirmed(&tx.tx_hash)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
                Ok(TxStatus::Queued) => {
                    info!(
                        "Transaction {} still queued, incrementing retry count to {}",
                        tx.tx_hash,
                        tx.retries + 1
                    );
                    self.db
                        .increment_queued_transaction_retry(&tx.tx_hash)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
                Ok(TxStatus::Dropped) => {
                    warn!("Transaction {} dropped", tx.tx_hash);
                    self.db
                        .mark_queued_transaction_dropped(&tx.tx_hash)
                        .await
                        .map_err(|e| QueuedTxMonitorError::DatabaseError(e.to_string()))?;
                }
                Err(e) => {
                    error!("Error checking transaction {}: {}", tx.tx_hash, e);
                    self.db
                        .increment_queued_transaction_retry(&tx.tx_hash)
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

    use crate::{
        client::MockXRPLClientTrait,
        queued_tx_monitor::{TxStatus, MAX_RETRIES},
        XrplQueuedTxMonitor,
    };
    use relayer_base::database::{MockDatabase, QueuedTransaction};
    use xrpl_api::{PaymentTransaction, Transaction, TransactionCommon, TxRequest, TxResponse};

    #[tokio::test]
    async fn test_check_transaction_status_confirmed() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_db = MockDatabase::new();

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

        let queued_tx_monitor = XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_db);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_ok());
        let tx_status = result.unwrap();
        match tx_status {
            TxStatus::Confirmed => {}
            _ => panic!("Expected TxStatus::Confirmed"),
        }
    }

    #[tokio::test]
    async fn test_check_transaction_status_queued() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_db = MockDatabase::new();

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

        let queued_tx_monitor = XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_db);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_ok());
        let tx_status = result.unwrap();
        match tx_status {
            TxStatus::Queued => {}
            _ => panic!("Expected TxStatus::Queued"),
        }
    }

    #[tokio::test]
    async fn test_check_transaction_status_dropped() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_db = MockDatabase::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(move |_| {
                Err(xrpl_http_client::error::Error::Api(
                    "txnNotFound".to_string(),
                ))
            });

        let queued_tx_monitor = XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_db);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_ok());
        let tx_status = result.unwrap();
        match tx_status {
            TxStatus::Dropped => {}
            _ => panic!("Expected TxStatus::Dropped"),
        }
    }

    #[tokio::test]
    async fn test_check_transaction_status_error() {
        let mut mock_client = MockXRPLClientTrait::new();
        let mock_db = MockDatabase::new();

        mock_client
            .expect_call::<TxRequest>()
            .times(1)
            .returning(move |_| {
                Err(xrpl_http_client::error::Error::Api(
                    "test error".to_string(),
                ))
            });

        let queued_tx_monitor = XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_db);
        let result = queued_tx_monitor
            .check_transaction_status("DUMMY_HASH")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_check_queued_transactions() {
        let mut mock_client = MockXRPLClientTrait::default();
        let mut mock_db = MockDatabase::new();

        mock_db
            .expect_get_queued_transactions_ready_for_check()
            .times(1)
            .returning(|| {
                Box::pin(async {
                    Ok(vec![
                        QueuedTransaction {
                            tx_hash: "DUMMY_HASH".to_string(),
                            retries: 0,
                        },
                        QueuedTransaction {
                            tx_hash: "DUMMY_HASH2".to_string(),
                            retries: 0,
                        },
                        // Expired
                        QueuedTransaction {
                            tx_hash: "DUMMY_HASH3".to_string(),
                            retries: MAX_RETRIES,
                        },
                        QueuedTransaction {
                            tx_hash: "DUMMY_HASH4".to_string(),
                            retries: 0,
                        },
                    ])
                })
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

        mock_db
            .expect_mark_queued_transaction_expired()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        mock_db
            .expect_mark_queued_transaction_confirmed()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        mock_db
            .expect_mark_queued_transaction_dropped()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        mock_db
            .expect_increment_queued_transaction_retry()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let queued_tx_monitor = XrplQueuedTxMonitor::new(Arc::new(mock_client), mock_db);

        let result = queued_tx_monitor.check_queued_transactions().await;
        assert!(result.is_ok());
    }
}
