/*!

Reads from TON blockchain and adds transactions to a queue.

# TODO:

- Also read failed executions/approvals and send them to the queue.

*/

use super::client::{RestClient, TONRpcClient};
use relayer_base::database::Database;
use relayer_base::error::SubscriberError;
use relayer_base::subscriber::{ChainTransaction, TransactionPoller};
use relayer_base::ton_types::Transaction;
use tonlib_core::TonAddress;
use tracing::{info, warn};

pub struct TONSubscriber<DB: Database> {
    client: TONRpcClient,
    latest_lt: i64,
    db: DB,
    context: String,
    chain_name: String,
}

impl<DB: Database> TONSubscriber<DB> {
    pub async fn new(
        url: String,
        ton_api_key: String,
        db: DB,
        context: String,
        chain_name: String,
    ) -> Result<Self, SubscriberError> {
        let client = TONRpcClient::new(url, 3, ton_api_key)
            .await
            .map_err(|e| error_stack::report!(SubscriberError::GenericError(e.to_string())))
            .unwrap();

        let latest_lt = db
            .get_latest_height(&chain_name, &context)
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))?
            .unwrap_or(-1);

        if latest_lt != -1 {
            info!("XRPL Subscriber: starting from ledger index: {}", latest_lt);
        }
        Ok(TONSubscriber {
            client,
            latest_lt,
            db,
            context,
            chain_name,
        })
    }

    async fn store_latest_height(&mut self) -> Result<(), SubscriberError> {
        self.db
            .store_latest_height(&self.chain_name, &self.context, self.latest_lt)
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))
    }
}

impl<DB: Database> TransactionPoller for TONSubscriber<DB> {
    type Transaction = Transaction;
    type Account = TonAddress;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::TON(tx)
    }

    async fn poll_account(
        &mut self,
        account_id: TonAddress,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let start_lt = if self.latest_lt == -1 {
            None
        } else {
            Some(self.latest_lt)
        };

        let transactions = self
            .client
            .get_transactions_for_account(account_id, start_lt)
            .await?;

        let max_lt = transactions.iter().map(|tx| tx.lt).max();

        if max_lt.is_some() {
            self.latest_lt = max_lt.unwrap_or(0);
            if let Err(err) = self.store_latest_height().await {
                warn!("{:?}", err);
            }
        }
        Ok(transactions)
    }

    async fn poll_tx(&mut self, _tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockRestClient;
    use mockall::predicate::eq;
    use relayer_base::database::MockDatabase;
    use relayer_base::ton_types::TransactionsResponse;
    use std::fs;

    #[tokio::test]
    async fn test_subscriber_no_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(None) }));

        let subscriber = TONSubscriber::new(
            "https://test-url".to_string(),
            "dummy-api-key".to_string(),
            mock_db,
            "test-context".to_string(),
            "test-chain".to_string(),
        )
        .await
        .expect("TONSubscriber should be created successfully");

        assert_eq!(subscriber.latest_lt, -1);
    }

    #[tokio::test]
    async fn test_subscriber_with_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(Some(12345)) }));

        let subscriber = TONSubscriber::new(
            "https://test-url".to_string(),
            "dummy-api-key".to_string(),
            mock_db,
            "test-context".to_string(),
            "test-chain".to_string(),
        )
        .await
        .expect("TONSubscriber should be created successfully");

        assert_eq!(subscriber.latest_lt, 12345);
    }

    #[tokio::test]
    async fn test_poll_account_with_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(Some(12345)) }));

        let mut mock_client = MockRestClient::new();

        let file_path = "tests/data/v3_transactions.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let transactions_response: TransactionsResponse =
            serde_json::from_str(&body).expect("Failed to deserialize test transaction data");

        let expected_transactions = transactions_response.transactions;

        mock_client
            .expect_get_transactions_for_account()
            .withf(|account, start_lt| {
                account.to_string() == "EQCqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqseb"
                    && *start_lt == Some(12345)
            })
            .returning(move |_, _| {
                let txs = expected_transactions.clone();
                Ok(txs)
            });
    }
}
