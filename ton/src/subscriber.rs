use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use super::client::TONRpcClient;
use relayer_base::database::Database;
use relayer_base::error::SubscriberError;
use tracing::{info, warn};
use relayer_base::subscriber::{ChainTransaction, TransactionPoller};
use relayer_base::ton_types::TONLogEvent;
use crate::approve_message::ApproveMessage;
use crate::broadcaster::TONTransaction;
use crate::types::TONLogEvent;

pub struct TONSubscriber<DB: Database> {
    client: TONRpcClient,
    latest_lt: i64,
    db: DB,
    context: String,
    chain_name: String,
}

impl<DB: Database> TONSubscriber<DB> {
    pub async fn new(
        url: &str,
        ton_api_key: &str,
        db: DB,
        context: String,
        chain_name: String,
    ) -> Result<Self, SubscriberError> {
        let client = TONRpcClient::new(url.to_string(), 3, ton_api_key.to_string())
            .await
            .map_err(|e| error_stack::report!(SubscriberError::GenericError(e.to_string()))).unwrap();

        let latest_lt = db
            .get_latest_height(&chain_name, &context)
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))?
            .unwrap_or(-1);

        if latest_lt != -1 {
            info!(
                "XRPL Subscriber: starting from ledger index: {}",
                latest_lt
            );
        }
        Ok(TONSubscriber {
            client,
            latest_lt,
            db,
            context,
            chain_name
        })
    }

    async fn store_latest_height(&mut self) -> Result<(), SubscriberError> {
        self.db
            .store_latest_height(&self.chain_name, &self.context, self.latest_lt)
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))
    }
}

// impl<DB: Database> TransactionPoller for TONSubscriber<DB> {
//     type Transaction = TONLogEvent;
// 
//     fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
//         ChainTransaction::TON(tx)
//     }
// 
//     async fn poll_account(
//         &mut self,
//         account_id: AccountId,
//     ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
//         let transactions = self
//             .client
//             .get_transactions_for_account(&account_id, self.latest_ledger as u32 + 1)
//             .await?;
// 
//         let max_response_ledger = transactions
//             .iter()
//             .map(|tx| tx.common().ledger_index.unwrap_or(0))
//             .max();
//         if max_response_ledger.is_some() {
//             self.latest_ledger = max_response_ledger.unwrap().into();
//             if let Err(err) = self.store_latest_ledger().await {
//                 warn!("{:?}", err);
//             }
//         }
//         Ok(transactions)
//     }
// 
//     async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
//         let request = xrpl_api::TxRequest::new(&tx_hash);
//         let res = self.client.call(request).await;
// 
//         let response = res.map_err(|e| anyhow!("Error getting tx: {:?}", e.to_string()))?;
// 
//         Ok(response.tx)
//     }
// }

#[cfg(test)]
mod tests {
    use mockall::predicate::eq;
    use relayer_base::database::MockDatabase;
    use super::*;

    #[tokio::test]
    async fn test_subscriber_no_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(None) }));

        let subscriber = TONSubscriber::new(
            "https://test-url",
            "dummy-api-key",
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
            "https://test-url",
            "dummy-api-key",
            mock_db,
            "test-context".to_string(),
            "test-chain".to_string(),
        )
            .await
            .expect("TONSubscriber should be created successfully");

        assert_eq!(subscriber.latest_lt, 12345);
    }

}
