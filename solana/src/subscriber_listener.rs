use std::{future::Future, str::FromStr, sync::Arc, time::Duration};

use crate::{
    client::{
        LogsSubscription, SolanaRpcClient, SolanaRpcClientTrait, SolanaStreamClient,
        SolanaStreamClientTrait,
    },
    config::SolanaConfig,
    models::solana_transaction::SolanaTransactionModel,
};

use crate::utils::upsert_and_publish;
use futures::StreamExt;
use relayer_base::{error::SubscriberError, queue::Queue};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_types::solana_types::SolanaTransaction;
use tokio::{select, time::timeout};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{error, info, warn};

pub trait TransactionListener {
    type Transaction;
    type Account;

    fn subscriber(
        &self,
        account: Self::Account,
    ) -> impl Future<Output = Result<LogsSubscription<'_>, anyhow::Error>>;

    fn unsubscribe(&self) -> impl Future<Output = ()>;
}

pub struct SolanaListener<STR: SolanaStreamClientTrait, SM: SolanaTransactionModel> {
    client: STR,
    transaction_model: Arc<SM>,
    queue: Arc<Queue>,
}

impl<STR: SolanaStreamClientTrait, SM: SolanaTransactionModel> TransactionListener
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

impl<STR: SolanaStreamClientTrait, SM: SolanaTransactionModel> SolanaListener<STR, SM> {
    pub async fn new(
        client: STR,
        transaction_model: Arc<SM>,
        queue: Arc<Queue>,
    ) -> Result<Self, SubscriberError> {
        Ok(SolanaListener {
            client,
            transaction_model,
            queue,
        })
    }

    pub async fn run(
        &self,
        gas_service_account: Pubkey,
        gateway_account: Pubkey,
        config: SolanaConfig,
        cancellation_token: CancellationToken,
    ) {
        let solana_stream_rpc_clone = config.clone().solana_stream_rpc;
        let solana_config_clone = config.clone();
        let queue_clone = Arc::clone(&self.queue);
        let transaction_model_clone = Arc::clone(&self.transaction_model);
        let cancellation_token_clone = cancellation_token.clone();

        let handle_gas_service = tokio::spawn(async move {
            Self::setup_connection_and_work(
                &solana_stream_rpc_clone,
                solana_config_clone,
                gas_service_account,
                &queue_clone,
                &transaction_model_clone,
                "gas_service",
                cancellation_token_clone,
            )
            .await;
        });

        tokio::pin!(handle_gas_service);

        let solana_stream_rpc_clone = config.clone().solana_stream_rpc;
        let solana_config_clone = config.clone();
        let queue_clone = Arc::clone(&self.queue);
        let transaction_model_clone = Arc::clone(&self.transaction_model);
        let cancellation_token_clone = cancellation_token.clone();

        let handle_gateway = tokio::spawn(async move {
            Self::setup_connection_and_work(
                &solana_stream_rpc_clone,
                solana_config_clone,
                gateway_account,
                &queue_clone,
                &transaction_model_clone,
                "gateway",
                cancellation_token_clone,
            )
            .await;
        });

        tokio::pin!(handle_gateway);

        tokio::select! {
            _ = &mut handle_gas_service => {
                info!("Gas service stopped");
            },
            _ = &mut handle_gateway => {
                info!("Gateway stopped");
            },

        }

        info!("Shut down solana listener");
    }

    async fn setup_connection_and_work(
        solana_stream_rpc: &str,
        solana_config: SolanaConfig,
        account: Pubkey,
        queue: &Arc<Queue>,
        transaction_model: &Arc<SM>,
        stream_name: &str,
        cancellation_token: CancellationToken,
    ) {
        loop {
            // TODO: Connection Pool
            let solana_stream_client =
                match SolanaStreamClient::new(solana_stream_rpc, solana_config.solana_commitment)
                    .await
                {
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

            let mut should_break = false;
            let tracker = TaskTracker::new();

            select! {
                _ = cancellation_token.cancelled() => {
                    warn!("Cancellation requested; no longer awaiting subscriber_stream.next()");
                    should_break = true;
                },
                _ = Self::work(
                    &mut subscriber_stream,
                stream_name,
                queue,
                transaction_model,
                &solana_config,
                cancellation_token.clone(),
                &tracker,
                ) => {}
            }

            tracker.close();
            tracker.wait().await;

            solana_stream_client.unsubscribe().await;

            drop(subscriber_stream);

            if let Err(e) = solana_stream_client.shutdown().await {
                error!("Error shutting down solana stream client: {:?}", e);
            }

            if should_break {
                break;
            }
        }
    }

    // TODO: Create a general stream work function and separate it from poll work
    async fn work(
        subscriber_stream: &mut LogsSubscription<'_>,
        stream_name: &str,
        queue: &Arc<Queue>,
        transaction_model: &Arc<SM>,
        solana_config: &SolanaConfig,
        cancellation_token: CancellationToken,
        tracker: &TaskTracker,
    ) {
        // Create a single RPC client and reuse it across spawned tasks to leverage
        // the underlying HTTP connection pooling, instead of creating a client per request.
        let solana_rpc_client = match SolanaRpcClient::new(
            &solana_config.solana_poll_rpc,
            solana_config.solana_commitment,
            3,
        ) {
            Ok(client) => Arc::new(client),
            Err(e) => {
                error!(
                    "Error creating solana rpc client for {}: {:?}",
                    stream_name, e
                );
                return;
            }
        };
        loop {
            info!("Waiting for messages from {}...", stream_name);
            // If the stream has not received any messages in 30 seconds, re-establish the connection to avoid silent failures
            select! {
                _ = cancellation_token.cancelled() => {
                    warn!("Cancellation requested; no longer awaiting subscriber_stream.next()");
                    break;
                },
                maybe_response = timeout(Duration::from_secs(30), subscriber_stream.next()) => {
                    match maybe_response {
                        Ok(Some(response)) => {
                            let signature = match Signature::from_str(&response.value.signature) {
                                Ok(signature) => signature,
                                Err(e) => {
                                    error!("Error parsing signature: {:?}", e);
                                    continue;
                                }
                            };

                            let transaction_model_clone = Arc::clone(transaction_model);
                            let queue_clone = Arc::clone(queue);
                            let stream_name_clone = stream_name.to_string();
                            let rpc_clone = Arc::clone(&solana_rpc_client);

                            tracker.spawn(async move {
                                let tx = match rpc_clone
                                .get_transaction_by_signature(signature)
                                .await
                            {
                                Ok(tx) => tx,
                                Err(e) => {
                                    error!("Error getting transaction by signature: {:?}", e);
                                    return;
                                }
                            };

                            match upsert_and_publish(&transaction_model_clone, &queue_clone, &tx, stream_name_clone.clone()).await {
                                Ok(inserted) => {
                                    if inserted {
                                        info!(
                                            "Upserted and published transaction by {} stream: {:?}",
                                            stream_name_clone, tx
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("Error upserting and publishing transaction: {:?}", e);
                                }
                            }
                            });


                        }
                        Ok(None) => {
                            warn!("Stream {} was closed", stream_name);
                            break;
                        }
                        Err(e) => {
                            warn!("Restarting {} stream: {:?}", stream_name, e);
                            break;
                        }
                    }
                }
            }
        }
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
