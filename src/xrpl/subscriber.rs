use anyhow::anyhow;
use tracing::{info, warn};
use xrpl_api::{RequestPagination, Transaction};
use xrpl_types::AccountId;

use crate::database::Database;
use crate::error::SubscriberError;
use crate::subscriber::TransactionPoller;

use super::client::XRPLClient;

pub struct XrplSubscriber<DB: Database> {
    client: XRPLClient,
    latest_ledger: i64,
    db: DB,
}

impl<DB: Database> XrplSubscriber<DB> {
    pub async fn new(url: &str, db: DB) -> Result<Self, SubscriberError> {
        let client = XRPLClient::new(url, 3).unwrap();

        let latest_ledger = db
            .get_latest_height("xrpl", "default")
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))?
            .unwrap_or(-1);

        if latest_ledger != -1 {
            info!(
                "XRPL Subscriber: starting from ledger index: {}",
                latest_ledger
            );
        }
        Ok(XrplSubscriber {
            client,
            latest_ledger,
            db,
        })
    }
}

impl<DB: Database> XrplSubscriber<DB> {
    async fn store_latest_ledger(&mut self) -> Result<(), anyhow::Error> {
        self.db
            .store_latest_height("xrpl", "default", self.latest_ledger)
            .await
            .map_err(|e| anyhow!("Error storing latest ledger: {:?}", e))
    }
}

impl<DB: Database> TransactionPoller for XrplSubscriber<DB> {
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
