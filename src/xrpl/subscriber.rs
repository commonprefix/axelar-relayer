use anyhow::anyhow;
use r2d2::{Pool, PooledConnection};
use redis::Commands;
use tracing::{info, warn};
use xrpl_api::{RequestPagination, Transaction};
use xrpl_types::AccountId;

use crate::subscriber::TransactionPoller;

use super::client::XRPLClient;

pub struct XrplSubscriber {
    client: XRPLClient,
    redis_conn: PooledConnection<redis::Client>,
    latest_ledger: i64,
}

impl XrplSubscriber {
    pub async fn new(url: &str, redis_pool: Pool<redis::Client>) -> Self {
        let client = XRPLClient::new(url, 3).unwrap();
        let mut redis_conn = redis_pool
            .get()
            .expect("Cannot get redis connection from pool");

        let latest_ledger = redis_conn
            .get("xrpl_subscriber:latest_ledger")
            .unwrap_or(-1);
        if latest_ledger != -1 {
            info!(
                "XRPL Subscriber: starting from ledger index: {}",
                latest_ledger
            );
        }
        XrplSubscriber {
            client,
            redis_conn,
            latest_ledger,
        }
    }
}

impl XrplSubscriber {
    async fn store_latest_ledger(&mut self) -> Result<(), anyhow::Error> {
        let _: () = self
            .redis_conn
            .set("xrpl_subscriber:latest_ledger", self.latest_ledger)
            .map_err(|e| {
                anyhow!(format!(
                    "Failed to store last_task_id to Redis: {}",
                    e.to_string()
                ))
            })?;
        Ok(())
    }
}

impl TransactionPoller for XrplSubscriber {
    type Transaction = Transaction;

    async fn poll_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let request = xrpl_api::AccountTxRequest {
            account: account_id.to_address(),
            forward: Some(true),
            ledger_index_min: Some((self.latest_ledger + 1).to_string()),
            ledger_index_max: Some((-1).to_string()),
            pagination: RequestPagination {
                limit: Some(50),
                ..Default::default()
            },
            ..Default::default()
        };
        let res = self.client.call(request).await;

        let response = res.map_err(|e| anyhow!("Error getting txs: {:?}", e.to_string()))?;

        let max_response_ledger = response
            .transactions
            .iter()
            .map(|tx| tx.tx.common().ledger_index.unwrap_or(0))
            .max();
        if max_response_ledger.is_some() {
            self.latest_ledger = max_response_ledger.unwrap().into();
            if let Err(err) = self.store_latest_ledger().await {
                warn!("{:?}", err);
            }
        }
        Ok(response.transactions.into_iter().map(|tx| tx.tx).collect())
    }

    async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        let request = xrpl_api::TxRequest::new(&tx_hash);
        let res = self.client.call(request).await;

        let response = res.map_err(|e| anyhow!("Error getting tx: {:?}", e.to_string()))?;

        Ok(response.tx)
    }
}
