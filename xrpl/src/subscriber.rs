use anyhow::anyhow;
use tracing::{info, warn};
use xrpl_api::Transaction;
use xrpl_types::AccountId;

use relayer_base::{
    database::Database,
    error::SubscriberError,
    subscriber::{ChainTransaction, TransactionPoller},
};

use crate::client::XRPLClientTrait;

pub struct XrplSubscriber<DB: Database, X: XRPLClientTrait> {
    client: X,
    latest_ledger: i64,
    db: DB,
    context: String,
}

impl<DB: Database, X: XRPLClientTrait> XrplSubscriber<DB, X> {
    pub async fn new(client: X, db: DB, context: String) -> Result<Self, SubscriberError> {
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

    #[tracing::instrument(skip(self))]
    pub async fn store_latest_ledger(&mut self) -> Result<(), anyhow::Error> {
        self.db
            .store_latest_height("xrpl", &self.context, self.latest_ledger)
            .await
            .map_err(|e| anyhow!("Error storing latest ledger: {:?}", e))
    }
}

impl<DB: Database, X: XRPLClientTrait> TransactionPoller for XrplSubscriber<DB, X> {
    type Transaction = Transaction;
    type Account = AccountId;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Xrpl(Box::new(tx))
    }

    fn transaction_id(&self, tx: &Self::Transaction) -> Option<String> {
        tx.common().hash.clone()
    }

    fn account_id(&self, account: &Self::Account) -> Option<String> {
        Some(account.to_address())
    }

    #[tracing::instrument(skip(self))]
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
            self.latest_ledger = max_response_ledger
                .ok_or(anyhow!("Max response ledger is None"))?
                .into();
            if let Err(err) = self.store_latest_ledger().await {
                warn!("{:?}", err);
            }
        }
        Ok(transactions)
    }

    #[tracing::instrument(skip(self))]
    async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        let request = xrpl_api::TxRequest::new(&tx_hash);
        let res = self.client.call(request).await;

        let response = res.map_err(|e| anyhow!("Error getting tx: {:?}", e.to_string()))?;

        Ok(response.tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrpl_api::{PaymentTransaction, Transaction, TransactionCommon};
    use xrpl_types::AccountId;

    // Simple test struct to test the transaction_id and account_id methods
    struct TestXrplPoller;

    impl TransactionPoller for TestXrplPoller {
        type Transaction = Transaction;
        type Account = AccountId;

        fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
            ChainTransaction::Xrpl(Box::new(tx))
        }

        fn transaction_id(&self, tx: &Self::Transaction) -> Option<String> {
            tx.common().hash.clone()
        }

        fn account_id(&self, account: &Self::Account) -> Option<String> {
            Some(account.to_address())
        }

        async fn poll_account(
            &mut self,
            _account: Self::Account,
        ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
            unimplemented!()
        }

        async fn poll_tx(&mut self, _tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
            unimplemented!()
        }
    }

    #[test]
    fn test_transaction_id() {
        let poller = TestXrplPoller;

        let tx_hash =
            "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF".to_string();
        let tx = Transaction::Payment(PaymentTransaction {
            common: TransactionCommon {
                hash: Some(tx_hash.clone()),
                ..Default::default()
            },
            ..Default::default()
        });

        let result = poller.transaction_id(&tx);

        assert_eq!(result, Some(tx_hash));
    }

    #[test]
    fn test_account_id() {
        let poller = TestXrplPoller;

        let account_id = AccountId::from_address("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh").unwrap();
        let result = poller.account_id(&account_id);

        assert_eq!(
            result,
            Some("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh".to_string())
        );
    }
}
