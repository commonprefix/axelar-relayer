use super::client::XRPLClient;
use relayer_base::{database::Database, error::QueuedTxMonitorError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use xrpl_api::TxRequest;
use xrpl_http_client;

const MAX_RETRIES: i32 = 20;
pub struct XrplQueuedTxMonitor<DB: Database> {
    client: Arc<XRPLClient>,
    db: DB,
}

impl<DB: Database> XrplQueuedTxMonitor<DB> {
    pub fn new(client: Arc<XRPLClient>, db: DB) -> Self {
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

pub enum TxStatus {
    Confirmed,
    Queued,
    Dropped,
}
