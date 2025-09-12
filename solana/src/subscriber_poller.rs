use std::{future::Future, str::FromStr, sync::Arc, time::Duration};

use crate::{
    client::SolanaRpcClientTrait,
    models::{
        solana_subscriber_cursor::{AccountPollerEnum, SubscriberCursor},
        solana_transaction::SolanaTransactionModel,
    },
    utils::upsert_and_publish,
};
use anyhow::anyhow;
use relayer_base::{error::SubscriberError, queue::Queue};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_types::solana_types::SolanaTransaction;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub trait TransactionPoller {
    type Transaction;
    type Account;

    fn poll_account(
        &self,
        account: Self::Account,
        account_type: AccountPollerEnum,
    ) -> impl Future<Output = Result<Vec<Self::Transaction>, anyhow::Error>>;

    fn poll_tx(
        &self,
        tx_hash: String,
    ) -> impl Future<Output = Result<Self::Transaction, anyhow::Error>>;
}

pub struct SolanaPoller<RPC: SolanaRpcClientTrait, SC: SubscriberCursor, SM: SolanaTransactionModel>
{
    client: RPC,
    cursor_model: Arc<SC>,
    context: String,
    transaction_model: Arc<SM>,
    queue: Arc<Queue>,
}

impl<RPC: SolanaRpcClientTrait, SC: SubscriberCursor, SM: SolanaTransactionModel>
    SolanaPoller<RPC, SC, SM>
{
    pub async fn new(
        client: RPC,
        context: String,
        transaction_model: Arc<SM>,
        cursor_model: Arc<SC>,
        queue: Arc<Queue>,
    ) -> Result<Self, SubscriberError> {
        Ok(SolanaPoller {
            client,
            context,
            cursor_model,
            transaction_model,
            queue,
        })
    }

    pub async fn run(
        &self,
        gas_service_account: Pubkey,
        gateway_account: Pubkey,
        cancellation_token: CancellationToken,
    ) {
        select! {
            _ = self.work(gas_service_account, AccountPollerEnum::GasService, cancellation_token.clone()) => {
                warn!("Gas service subscriber stream ended");
            },
            _ = self.work(gateway_account, AccountPollerEnum::Gateway, cancellation_token.clone()) => {
                warn!("Gateway subscriber stream ended");
            }
        };
    }

    // Poller runs in the background and polls every X seconds for all transactions
    // that happened for the specified account since its last poll. It only writes
    // to the queue if the transaction has not been processed already.
    // It acts as a backup to the listener for when it's initiated or restarted to not
    // miss any transactions.

    // TODO: Create a general stream work function and separate it from poll work
    async fn work(
        &self,
        account: Pubkey,
        account_type: AccountPollerEnum,
        cancellation_token: CancellationToken,
    ) {
        loop {
            if cancellation_token.is_cancelled() {
                warn!("Cancellation requested; no longer polling account");
                break;
            }
            let transactions = match self.poll_account(account, account_type.clone()).await {
                Ok(transactions) => transactions,
                Err(e) => {
                    error!("Error polling account: {:?}", e);
                    continue;
                }
            };

            for tx in transactions.clone() {
                match upsert_and_publish(
                    &self.transaction_model,
                    &self.queue,
                    &tx,
                    "poller".to_string(),
                )
                .await
                {
                    Ok(inserted) => {
                        if inserted {
                            info!("Upserted and published transaction by poller: {:?}", tx);
                        }
                    }
                    Err(e) => {
                        error!("Error upserting and publishing transaction: {:?}", e);
                        continue;
                    }
                }
            }
            let maybe_transaction_with_max_slot = transactions.iter().max_by_key(|tx| tx.slot);
            if let Some(transaction) = maybe_transaction_with_max_slot {
                if let Err(err) = self
                    .store_last_transaction_checked(transaction.signature, account_type.clone())
                    .await
                {
                    error!("{:?}", err);
                    continue;
                }
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    pub async fn store_last_transaction_checked(
        &self,
        signature: Signature,
        account_type: AccountPollerEnum,
    ) -> Result<(), anyhow::Error> {
        self.cursor_model
            .store_latest_signature(self.context.clone(), signature.to_string(), account_type)
            .await
            .map_err(|e| anyhow!("Error storing latest signature: {:?}", e))
    }

    pub async fn get_last_signature_checked(
        &self,
        account_type: AccountPollerEnum,
    ) -> Result<Option<Signature>, anyhow::Error> {
        match self
            .cursor_model
            .get_latest_signature(self.context.clone(), account_type)
            .await
            .map_err(|e| anyhow!("Error getting latest signature: {:?}", e))?
        {
            Some(signature) => {
                let maybe_sig = match Signature::from_str(&signature) {
                    Ok(sig) => Some(sig),
                    Err(e) => {
                        error!("Error parsing signature: {:?}", e);
                        None
                    }
                };
                Ok(maybe_sig)
            }
            None => Ok(None),
        }
    }
}

impl<RPC: SolanaRpcClientTrait, SC: SubscriberCursor, SM: SolanaTransactionModel> TransactionPoller
    for SolanaPoller<RPC, SC, SM>
{
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    async fn poll_account(
        &self,
        account_id: Pubkey,
        account_type: AccountPollerEnum,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let last_signature_checked = self
            .get_last_signature_checked(account_type.clone())
            .await?;
        let transactions = self
            .client
            .get_transactions_for_account(&account_id, None, last_signature_checked)
            .await?;

        Ok(transactions)
    }

    async fn poll_tx(&self, signature: String) -> Result<Self::Transaction, anyhow::Error> {
        let transaction = self
            .client
            .get_transaction_by_signature(Signature::from_str(&signature)?)
            .await?;

        Ok(transaction)
    }
}

// comment out tests to not spam RPC on every push

// #[cfg(test)]
// mod tests {

//     use std::str::FromStr;

//     use dotenv::dotenv;
//     use relayer_base::config::config_from_yaml;
//     use solana_sdk::commitment_config::CommitmentConfig;
//     use testcontainers::{runners::AsyncRunner, ContainerAsync};
//     use testcontainers_modules::postgres;

//     use crate::{
//         client::SolanaRpcClient, config::SolanaConfig,
//         models::solana_subscriber_cursor::PostgresDB, solana_transaction::PgSolanaTransactionModel,
//     };

//     use super::*;

//     async fn setup_test_container() -> (
//         PostgresDB,
//         PgSolanaTransactionModel,
//         Arc<Queue>,
//         ContainerAsync<postgres::Postgres>,
//     ) {
//         dotenv().ok();
//         let init_sql = format!(
//             "{}\n{}",
//             include_str!("../../migrations/0015_solana_subscriber_cursors.sql"),
//             include_str!("../../migrations/0014_solana_transactions.sql")
//         );
//         let container = postgres::Postgres::default()
//             .with_init_sql(init_sql.into_bytes())
//             .start()
//             .await
//             .unwrap();
//         let connection_string = format!(
//             "postgres://postgres:postgres@{}:{}/postgres",
//             container.get_host().await.unwrap(),
//             container.get_host_port_ipv4(5432).await.unwrap()
//         );
//         let pool = sqlx::PgPool::connect(&connection_string).await.unwrap();
//         let cursor_model = PostgresDB::new(&connection_string).await.unwrap();
//         let transaction_model = PgSolanaTransactionModel::new(pool);
//         let queue = Queue::new("amqp://guest:guest@localhost:5672", "test", 1).await;

//         (cursor_model, transaction_model, queue, container)
//     }

//     #[tokio::test]
//     async fn test_poll_account() {
//         let (cursor_model, transaction_model, queue, _container) = setup_test_container().await;
//         let network = std::env::var("NETWORK").expect("NETWORK must be set");
//         let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();
//         let solana_client: SolanaRpcClient =
//             SolanaRpcClient::new(&config.solana_poll_rpc, CommitmentConfig::confirmed(), 3)
//                 .unwrap();
//         let solana_subscriber = SolanaPoller::new(
//             solana_client,
//             "test".to_string(),
//             Arc::new(transaction_model),
//             Arc::new(cursor_model),
//             queue,
//         )
//         .await
//         .unwrap();

//         let _transactions = solana_subscriber
//             .poll_account(
//                 Pubkey::from_str("DaejccUfXqoAFTiDTxDuMQfQ9oa6crjtR9cT52v1AvGK").unwrap(),
//                 AccountPollerEnum::GasService,
//             )
//             .await
//             .unwrap();
//     }
// }
