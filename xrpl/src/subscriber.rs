use anyhow::anyhow;
use tracing::{info, warn};
use xrpl_api::Transaction;
use xrpl_types::AccountId;

use relayer_base::{
    database::Database,
    error::SubscriberError,
    subscriber::{ChainTransaction, TransactionPoller},
};

use super::client::XRPLClient;

pub struct XrplSubscriber<DB: Database> {
    client: XRPLClient,
    latest_ledger: i64,
    db: DB,
    context: String,
}

impl<DB: Database> XrplSubscriber<DB> {
    pub async fn new(url: &str, db: DB, context: String) -> Result<Self, SubscriberError> {
        let client = XRPLClient::new(url, 3).unwrap();

        let latest_ledger = db
            .get_latest_height("xrpl", &context)
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
            context,
        })
    }

    pub async fn store_latest_ledger(&mut self) -> Result<(), anyhow::Error> {
        self.db
            .store_latest_height("xrpl", &self.context, self.latest_ledger)
            .await
            .map_err(|e| anyhow!("Error storing latest ledger: {:?}", e))
    }
}

impl<DB: Database> TransactionPoller for XrplSubscriber<DB> {
    type Transaction = Transaction;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Xrpl(tx)
    }

    async fn poll_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let transactions = self
            .client
            .get_transactions_for_account(&account_id, (self.latest_ledger + 1) as u32)
            .await?;

        let max_response_ledger = transactions
            .iter()
            .map(|tx| tx.common().ledger_index.unwrap_or(0))
            .max();
        if max_response_ledger.is_some() {
            self.latest_ledger = max_response_ledger.unwrap().into();
            if let Err(err) = self.store_latest_ledger().await {
                warn!("{:?}", err);
            }
        }
        Ok(transactions)
    }

    async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        let request = xrpl_api::TxRequest::new(&tx_hash);
        let res = self.client.call(request).await;

        let response = res.map_err(|e| anyhow!("Error getting tx: {:?}", e.to_string()))?;

        Ok(response.tx)
    }
}
