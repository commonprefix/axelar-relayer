use std::{future::Future, str::FromStr, sync::Arc, time::Duration};

use crate::{
    client::{LogsSubscription, SolanaRpcClientTrait, SolanaStreamClient, SolanaStreamClientTrait},
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
use solana_sdk::signature::Signature;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_types::solana_types::SolanaTransaction;
use tokio::{
    select,
    signal::unix::{signal, SignalKind},
    task::JoinHandle,
};
use tracing::{debug, error, info, warn};

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

pub trait TransactionListener {
    type Transaction;
    type Account;

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

    pub async fn run(
        &self,
        gas_service_account: Pubkey,
        gateway_account: Pubkey,
        config: SolanaConfig,
    ) {
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(sigint) => sigint,
            Err(e) => {
                error!("Error creating interrupt signal: {:?}", e);
                return;
            }
        };
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(sigterm) => sigterm,
            Err(e) => {
                error!("Error creating terminate signal: {:?}", e);
                return;
            }
        };

        let mut handles: Vec<JoinHandle<()>> = vec![];

        let solana_stream_rpc_clone = config.clone().solana_stream_rpc;
        let solana_commitment_clone = config.clone().solana_commitment;
        let queue_clone = Arc::clone(&self.queue);
        let signature_model_clone = Arc::clone(&self.signature_model);

        handles.push(tokio::spawn(async move {
            Self::setup_connection_and_work(
                &solana_stream_rpc_clone,
                solana_commitment_clone,
                gas_service_account,
                &queue_clone,
                &signature_model_clone,
                "gas_service",
            )
            .await;
        }));

        let solana_stream_rpc_clone = config.clone().solana_stream_rpc;
        let solana_commitment_clone = config.clone().solana_commitment;
        let queue_clone = Arc::clone(&self.queue);
        let signature_model_clone = Arc::clone(&self.signature_model);

        handles.push(tokio::spawn(async move {
            Self::setup_connection_and_work(
                &solana_stream_rpc_clone,
                solana_commitment_clone,
                gateway_account,
                &queue_clone,
                &signature_model_clone,
                "gateway",
            )
            .await;
        }));

        tokio::select! {
            _ = sigint.recv()  => {},
            _ = sigterm.recv() => {},
        }

        for handle in handles {
            handle.abort();
        }

        info!("Shut down solana listener");
    }

    async fn setup_connection_and_work(
        solana_stream_rpc: &str,
        solana_commitment: CommitmentConfig,
        account: Pubkey,
        queue: &Arc<Queue>,
        signature_model: &Arc<SM>,
        stream_name: &str,
    ) {
        loop {
            let solana_stream_client =
                match SolanaStreamClient::new(solana_stream_rpc, solana_commitment).await {
                    Ok(solana_stream_client) => solana_stream_client,
                    Err(e) => {
                        error!(
                            "Error creating solana stream client for {}: {:?}",
                            stream_name, e
                        );
                        break;
                    }
                };

            let mut subscriber_stream = match solana_stream_client
                .logs_subscriber(account.to_string())
                .await
            {
                Ok(subscriber) => subscriber,
                Err(e) => {
                    error!("Error creating {} subscriber stream: {:?}", stream_name, e);
                    break;
                }
            };
            Self::work(&mut subscriber_stream, stream_name, queue, signature_model).await;

            solana_stream_client.unsubscribe().await;

            drop(subscriber_stream);

            if let Err(e) = solana_stream_client.shutdown().await {
                error!("Error shutting down solana stream client: {:?}", e);
            }
        }
    }

    // TODO: Create a general stream work function and separate it from poll work
    async fn work(
        subscriber_stream: &mut LogsSubscription<'_>,
        stream_name: &str,
        queue: &Arc<Queue>,
        signature_model: &Arc<SM>,
    ) {
        loop {
            info!("Waiting for messages from {}...", stream_name);
            // If the stream has not received any messages in 30 seconds, re-establish the connection to avoid silent failures
            select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    warn!("Restarting {} stream", stream_name);
                    break;
                }
                maybe_response = subscriber_stream.next() => {
                    // TODO : Spawn a task to handle the response
                    match maybe_response {
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

                            match upsert_and_publish(
                                signature_model,
                                queue,
                                &tx,
                                stream_name,
                            )
                            .await
                            {
                                Ok(inserted) => {
                                    if inserted {
                                        info!(
                                            "Upserted and published transaction by {} stream: {:?}",
                                            stream_name, tx
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("Error upserting and publishing transaction: {:?}", e);
                                    continue;
                                }
                            }
                        }
                        None => {
                            warn!("Stream {} was closed", stream_name);
                            break;
                        }
                    }
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
    // that happened for the specified account since its last poll. It only writes
    // to the queue if the transaction has not been processed already.
    // It acts as a backup to the listener for when it's initiated or restarted to not
    // miss any transactions.

    // TODO: Create a general stream work function and separate it from poll work
    async fn work(&self, account: Pubkey, account_type: AccountPollerEnum) {
        loop {
            let transactions = match self.poll_account(account, account_type.clone()).await {
                Ok(transactions) => transactions,
                Err(e) => {
                    error!("Error polling account: {:?}", e);
                    continue;
                }
            };

            for tx in transactions.clone() {
                match upsert_and_publish(&self.signature_model, &self.queue, &tx, "poller").await {
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

async fn upsert_and_publish<SM: SolanaSignatureModel>(
    signature_model: &Arc<SM>,
    queue: &Arc<Queue>,
    tx: &SolanaTransaction,
    from_service: &str,
) -> Result<bool, anyhow::Error> {
    let inserted = signature_model
        .upsert(SolanaSignature {
            signature: tx.signature.to_string(),
            created_at: Utc::now(),
        })
        .await
        .map_err(|e| anyhow!("Error upserting transaction: {:?}", e))?;

    if inserted {
        let chain_transaction = ChainTransaction::Solana(Box::new(tx.clone()));

        let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
        debug!(
            "Publishing transaction from {}: {:?}",
            from_service, chain_transaction
        );
        queue.publish(item.clone()).await;
    } else {
        debug!("Transaction already exists: {:?}", tx.signature);
    }
    Ok(inserted)
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
//         models::solana_subscriber_cursor::PostgresDB, solana_signature::PgSolanaSignatureModel,
//     };

//     use super::*;

//     async fn setup_test_container() -> (
//         PostgresDB,
//         PgSolanaSignatureModel,
//         Arc<Queue>,
//         ContainerAsync<postgres::Postgres>,
//     ) {
//         dotenv().ok();
//         let init_sql = format!(
//             "{}\n{}",
//             include_str!("../../migrations/0015_solana_subscriber_cursors.sql"),
//             include_str!("../../migrations/0014_solana_signatures.sql")
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
//         let transaction_model = PgSolanaSignatureModel::new(pool);
//         let queue = Queue::new("amqp://guest:guest@localhost:5672", "test").await;

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
