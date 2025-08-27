use std::{future::Future, str::FromStr, sync::Arc, time::Duration};

use crate::{
    client::{
        LogsSubscription, SolanaRpcClient, SolanaRpcClientTrait, SolanaStreamClient,
        SolanaStreamClientTrait,
    },
    config::SolanaConfig,
    models::{
        solana_signature::SolanaSignatureModel,
        solana_subscriber_cursor::{AccountPollerEnum, SubscriberCursor},
    },
    solana_signature::SolanaSignature,
    utils::create_solana_tx_from_rpc_response,
};
use anyhow::anyhow;
use chrono::Utc;
use futures::StreamExt;
use relayer_base::queue::QueueItem;
use relayer_base::{error::SubscriberError, queue::Queue, subscriber::ChainTransaction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_types::solana_types::SolanaTransaction;
use tokio::select;
use tracing::{debug, error, info, warn};

pub trait TransactionPoller {
    type Transaction;
    type Account;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction;

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

pub trait TransactionListener {
    type Transaction;
    type Account;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction;

    fn subscriber(
        &self,
        account: Self::Account,
    ) -> impl Future<Output = Result<LogsSubscription<'_>, anyhow::Error>>;

    fn unsubscribe(&self) -> impl Future<Output = ()>;
}

pub struct SolanaPoller<RPC: SolanaRpcClientTrait, SC: SubscriberCursor, SM: SolanaSignatureModel> {
    client: RPC,
    cursor_model: Arc<SC>,
    context: String,
    signature_model: Arc<SM>,
    queue: Arc<Queue>,
}

pub struct SolanaListener<STR: SolanaStreamClientTrait, SM: SolanaSignatureModel> {
    client: STR,
    signature_model: Arc<SM>,
    queue: Arc<Queue>,
}

impl<STR: SolanaStreamClientTrait, SM: SolanaSignatureModel> TransactionListener
    for SolanaListener<STR, SM>
{
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Box::new(tx))
    }

    async fn subscriber(
        &self,
        account: Self::Account,
    ) -> Result<LogsSubscription<'_>, anyhow::Error> {
        self.client.logs_subscriber(account.to_string()).await
    }

    async fn unsubscribe(&self) {
        self.client.unsubscribe().await;
    }
}

impl<STR: SolanaStreamClientTrait, SM: SolanaSignatureModel> SolanaListener<STR, SM> {
    pub async fn new(
        client: STR,
        signature_model: Arc<SM>,
        queue: Arc<Queue>,
    ) -> Result<Self, SubscriberError> {
        Ok(SolanaListener {
            client,
            signature_model,
            queue,
        })
    }

    pub async fn run(&self, gas_service_account: Pubkey, gateway_account: Pubkey) {
        loop {
            let mut gas_service_subscriber_stream =
                match self.subscriber(gas_service_account.clone()).await {
                    Ok(consumer) => consumer,
                    Err(e) => {
                        error!("Failed to create subscriber stream: {:?}", e);
                        return;
                    }
                };
            let mut gateway_subscriber_stream = match self.subscriber(gateway_account.clone()).await
            {
                Ok(consumer) => consumer,
                Err(e) => {
                    error!("Failed to create subscriber stream: {:?}", e);
                    return;
                }
            };

            select! {
                _ = self.work(&mut gas_service_subscriber_stream, "gas_service") => {
                    warn!("Gas service subscriber stream ended");
                },
                _ = self.work(&mut gateway_subscriber_stream, "gateway") => {
                    warn!("Gateway subscriber stream ended");
                }
            };
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    // TODO: Create a general stream work function and seperate it from poll work
    async fn work(&self, subscriber_stream: &mut LogsSubscription<'_>, stream_name: &str) {
        loop {
            info!("Waiting for messages from {}...", stream_name);
            match subscriber_stream.next().await {
                Some(response) => {
                    let tx = match create_solana_tx_from_rpc_response(response.clone()) {
                        Ok(tx) => tx,
                        Err(e) => {
                            error!(
                                "Error creating solana transaction: {:?} from response: {:?}",
                                e, response
                            );
                            continue;
                        }
                    };

                    match self
                        .signature_model
                        .upsert(SolanaSignature {
                            signature: tx.signature.to_string(),
                            created_at: Utc::now(),
                        })
                        .await
                    {
                        Ok(inserted) => {
                            if inserted {
                                let chain_transaction =
                                    ChainTransaction::Solana(Box::new(tx.clone()));

                                let item =
                                    &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                                info!("Publishing transaction: {:?}", chain_transaction);
                                self.queue.publish(item.clone()).await;
                                debug!("Published tx: {:?}", item);
                            } else {
                                debug!("Transaction already exists: {:?}", tx.signature);
                            }
                        }
                        Err(e) => {
                            error!("Error upserting transaction: {:?}", e);
                            continue;
                        }
                    }
                }
                None => {
                    warn!("Stream was closed");
                    break;
                }
            }
        }
    }
}

impl<RPC: SolanaRpcClientTrait, SC: SubscriberCursor, SM: SolanaSignatureModel>
    SolanaPoller<RPC, SC, SM>
{
    pub async fn new(
        client: RPC,
        context: String,
        signature_model: Arc<SM>,
        cursor_model: Arc<SC>,
        queue: Arc<Queue>,
    ) -> Result<Self, SubscriberError> {
        Ok(SolanaPoller {
            client,
            context,
            cursor_model,
            signature_model,
            queue,
        })
    }

    pub async fn run(&self, gas_service_account: Pubkey, gateway_account: Pubkey) {
        loop {
            select! {
                _ = self.work(gas_service_account, AccountPollerEnum::GasService) => {
                    warn!("Gas service subscriber stream ended");
                },
                _ = self.work(gateway_account, AccountPollerEnum::Gateway) => {
                    warn!("Gateway subscriber stream ended");
                }
            };
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    // Poller runs in the background and polls every X seconds for all transactions
    // that happened for the specified account since its last poll. It only wriites
    // to the queue if the transaction has not been processed already.
    // It acts as a backup to the listener for when it's initiated or restarted to not
    // miss any transactions.

    // TODO: Create a general stream work function and seperate it from poll work
    async fn work(&self, account: Pubkey, account_type: AccountPollerEnum) {
        loop {
            let transactions = self
                .poll_account(account, account_type.clone())
                .await
                .unwrap();
            for tx in transactions.clone() {
                match self
                    .signature_model
                    .upsert(SolanaSignature {
                        signature: tx.signature.to_string(),
                        created_at: Utc::now(),
                    })
                    .await
                {
                    Ok(inserted) => {
                        if inserted {
                            let chain_transaction = ChainTransaction::Solana(Box::new(tx.clone()));

                            let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                            info!("Publishing transaction: {:?}", chain_transaction);
                            self.queue.publish(item.clone()).await;
                            debug!("Published tx: {:?}", item);
                        } else {
                            debug!("Transaction already exists: {:?}", tx.signature);
                        }
                    }
                    Err(e) => {
                        error!("Error upserting transaction: {:?}", e);
                        continue;
                    }
                }
            }
            let maybe_transaction_with_max_slot = transactions.iter().max_by_key(|tx| tx.slot);
            if let Some(transaction) = maybe_transaction_with_max_slot {
                if let Err(err) = self
                    .store_last_signature_checked(transaction.signature, account_type.clone())
                    .await
                {
                    error!("{:?}", err);
                    continue;
                }
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    pub async fn store_last_signature_checked(
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

impl<RPC: SolanaRpcClientTrait, SC: SubscriberCursor, SM: SolanaSignatureModel> TransactionPoller
    for SolanaPoller<RPC, SC, SM>
{
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Box::new(tx))
    }

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
            .get_transaction_by_signature(Signature::from_str(&signature).unwrap())
            .await?;

        Ok(transaction)
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    use dotenv::dotenv;
    use relayer_base::config::config_from_yaml;
    use solana_sdk::commitment_config::CommitmentConfig;
    use testcontainers::{runners::AsyncRunner, ContainerAsync};
    use testcontainers_modules::postgres;

    use crate::{
        client::SolanaRpcClient, config::SolanaConfig,
        models::solana_subscriber_cursor::PostgresDB, solana_signature::PgSolanaSignatureModel,
    };

    use super::*;

    async fn setup_test_container() -> (
        PostgresDB,
        PgSolanaSignatureModel,
        Arc<Queue>,
        ContainerAsync<postgres::Postgres>,
    ) {
        dotenv().ok();
        let init_sql = format!(
            "{}\n{}",
            include_str!("../../migrations/0015_solana_subscriber_cursors.sql"),
            include_str!("../../migrations/0014_solana_signatures.sql")
        );
        let container = postgres::Postgres::default()
            .with_init_sql(init_sql.into_bytes())
            .start()
            .await
            .unwrap();
        let connection_string = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await.unwrap(),
            container.get_host_port_ipv4(5432).await.unwrap()
        );
        let pool = sqlx::PgPool::connect(&connection_string).await.unwrap();
        let cursor_model = PostgresDB::new(&connection_string).await.unwrap();
        let transaction_model = PgSolanaSignatureModel::new(pool);
        let queue = Queue::new("amqp://guest:guest@localhost:5672", "test").await;

        (cursor_model, transaction_model, queue, container)
    }

    #[tokio::test]
    async fn test_poll_account() {
        let (cursor_model, transaction_model, queue, _container) = setup_test_container().await;
        let network = std::env::var("NETWORK").expect("NETWORK must be set");
        let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();
        let solana_client: SolanaRpcClient =
            SolanaRpcClient::new(&config.solana_poll_rpc, CommitmentConfig::confirmed(), 3)
                .unwrap();
        let solana_subscriber = SolanaPoller::new(
            solana_client,
            "test".to_string(),
            Arc::new(transaction_model),
            Arc::new(cursor_model),
            queue,
        )
        .await
        .unwrap();

        let transactions = solana_subscriber
            .poll_account(
                Pubkey::from_str("DaejccUfXqoAFTiDTxDuMQfQ9oa6crjtR9cT52v1AvGK").unwrap(),
                AccountPollerEnum::GasService,
            )
            .await
            .unwrap();

        println!("{:?}", transactions.len());
        //assert_eq!(transactions.len(), 18);
    }
}
