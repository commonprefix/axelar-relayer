use std::{future::Future, str::FromStr, sync::Arc};

use crate::{
    client::{
        LogsSubscription, SolanaRpcClient, SolanaRpcClientTrait, SolanaStreamClient,
        SolanaStreamClientTrait,
    },
    config::SolanaConfig,
    models::{
        solana_subscriber_cursor::{SubscriberCursor, SubscriberMode},
        solana_transaction::SolanaTransactionModel,
    },
    utils::create_solana_tx_from_rpc_response,
};
use anyhow::anyhow;
use futures::StreamExt;
use relayer_base::queue::QueueItem;
use relayer_base::{
    error::SubscriberError,
    queue::Queue,
    subscriber::{ChainTransaction, TransactionPoller},
};
use solana_sdk::signature::Signature;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_types::solana_types::SolanaTransaction;
use tokio::select;
use tracing::{debug, error, info, warn};

pub struct SolanaSubscriber<
    SC: SubscriberCursor,
    STM: SolanaTransactionModel,
    RPC: SolanaRpcClientTrait,
    STR: SolanaStreamClientTrait,
> {
    rpc_subscriber: SolanaPoller<RPC>,
    stream_subscriber: SolanaListener<STR>,
    cursor_model: Arc<SC>,
    transaction_model: Arc<STM>,
    context: String,
}

pub struct SolanaPoller<RPC: SolanaRpcClientTrait> {
    client: RPC,
    last_signature_checked: Option<Signature>,
}

pub struct SolanaListener<STR: SolanaStreamClientTrait> {
    client: STR,
    last_signature_checked: Option<Signature>,
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

impl<
        SC: SubscriberCursor,
        STM: SolanaTransactionModel,
        RPC: SolanaRpcClientTrait,
        STR: SolanaStreamClientTrait,
    > SolanaSubscriber<SC, STM, RPC, STR>
{
    pub async fn new(
        cursor_model: SC,
        transaction_model: STM,
        context: String,
        config: SolanaConfig,
    ) -> Result<Self, anyhow::Error> {
        let rpc_client = SolanaRpcClient::new(&config.solana_rpc, config.solana_commitment, 3)?;
        let stream_client =
            SolanaStreamClient::new(&config.solana_rpc, config.solana_commitment).await?;
        let maybe_latest_signature_checked = cursor_model
            .get_latest_signature(context.clone(), SubscriberMode::Poll)
            .await?;
        let maybe_latest_signature_checked = cursor_model
            .get_latest_signature(context.clone(), SubscriberMode::Stream)
            .await?;
        let solana_poller = SolanaPoller::new(rpc_client, maybe_latest_signature_checked).await?;
        let solana_listener =
            SolanaListener::new(stream_client, maybe_latest_signature_checked).await?;
        Ok(Self {
            rpc_subscriber: solana_poller,
            stream_subscriber: solana_listener,
            cursor_model: Arc::new(cursor_model),
            transaction_model: Arc::new(transaction_model),
            context,
        })
    }
}

impl<STR: SolanaStreamClientTrait> SolanaListener<STR> {
    pub async fn new(
        client: STR,
        maybe_latest_signature_checked: Option<String>,
    ) -> Result<Self, SubscriberError> {
        let last_signature_checked = match maybe_latest_signature_checked {
            Some(signature) => {
                let maybe_sig = match Signature::from_str(&signature) {
                    Ok(sig) => Some(sig),
                    Err(e) => {
                        error!("Error parsing signature: {:?}", e);
                        None
                    }
                };
                maybe_sig
            }
            None => None,
        };

        Ok(SolanaListener {
            client,
            last_signature_checked,
        })
    }

    pub async fn run(
        &self,
        gas_service_account: Pubkey,
        gateway_account: Pubkey,
        queue: Arc<Queue>,
        subscriber_cursor: Arc<impl SubscriberCursor>,
    ) {
        let mut gas_service_subscriber_stream =
            match self.subscriber(gas_service_account.clone()).await {
                Ok(consumer) => consumer,
                Err(e) => {
                    error!("Failed to create subscriber stream: {:?}", e);
                    return;
                }
            };
        let mut gateway_subscriber_stream = match self.subscriber(gateway_account.clone()).await {
            Ok(consumer) => consumer,
            Err(e) => {
                error!("Failed to create subscriber stream: {:?}", e);
                return;
            }
        };

        select! {
            _ = self.work_stream(&mut gas_service_subscriber_stream, "gas_service", Arc::clone(&queue), Arc::clone(&subscriber_cursor)) => {
                warn!("Gas service subscriber stream ended");
            },
            _ = self.work_stream(&mut gateway_subscriber_stream, "gateway", Arc::clone(&queue), Arc::clone(&subscriber_cursor)) => {
                warn!("Gateway subscriber stream ended");
            }
        };
        // TODO reconnect here
    }

    // TODO: Create a general stream work function and seperate it from poll work
    async fn work_stream(
        &self,
        subscriber_stream: &mut LogsSubscription<'_>,
        stream_name: &str,
        queue: Arc<Queue>,
        subscriber_cursor: Arc<impl SubscriberCursor>,
    ) {
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
                    let chain_transaction = ChainTransaction::Solana(Box::new(tx.clone()));

                    let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                    info!("Publishing transaction: {:?}", chain_transaction);
                    queue.publish(item.clone()).await;
                    if let Err(e) = subscriber_cursor
                        .store_latest_signature(
                            stream_name.to_string(),
                            tx.signature.to_string(),
                            SubscriberMode::Stream,
                        )
                        .await
                    {
                        error!("Error storing latest signature: {:?}", e);
                    }
                    debug!("Published tx: {:?}", item);
                }
                None => {
                    warn!("Stream was closed");
                    break;
                }
            }
        }
    }

    async fn work_poll(
        &self,
        account: Pubkey,
        queue: Arc<Queue>,
        subscriber_cursor: Arc<impl SubscriberCursor>,
    ) {
        loop {}
    }
}

impl<RPC: SolanaRpcClientTrait> SolanaPoller<RPC> {
    pub async fn new(
        client: RPC,
        maybe_latest_signature_checked: Option<String>,
    ) -> Result<Self, SubscriberError> {
        let last_signature_checked = match maybe_latest_signature_checked {
            Some(signature) => {
                let maybe_sig = match Signature::from_str(&signature) {
                    Ok(sig) => Some(sig),
                    Err(e) => {
                        error!("Error parsing signature: {:?}", e);
                        None
                    }
                };
                maybe_sig
            }
            None => None,
        };

        Ok(SolanaPoller {
            client,
            last_signature_checked,
        })
    }

    // pub async fn store_last_signature_checked(
    //     &mut self,
    //     mode: SubscriberMode,
    // ) -> Result<(), anyhow::Error> {
    //     match self.last_signature_checked {
    //         Some(signature) => self
    //             .cursor_model
    //             .store_latest_signature(self.context.clone(), signature.to_string(), mode)
    //             .await
    //             .map_err(|e| anyhow!("Error storing latest signature: {:?}", e)),
    //         None => Err(anyhow!("No signature to store")),
    //     }
    // }
}

impl<RPC: SolanaRpcClientTrait> TransactionPoller for SolanaPoller<RPC> {
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Box::new(tx))
    }

    async fn poll_account(
        &mut self,
        account_id: Pubkey,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let transactions = self
            .client
            .get_transactions_for_account(&account_id, None, self.last_signature_checked)
            .await?;

        let maybe_transaction_with_max_slot = transactions.iter().max_by_key(|tx| tx.slot);
        if let Some(transaction) = maybe_transaction_with_max_slot {
            self.last_signature_checked = Some(transaction.signature);
            // if let Err(err) = self
            //     .store_last_signature_checked(SubscriberMode::Poll)
            //     .await
            // {
            //     error!("{:?}", err);
            //     return Err(err);
            // }
        }
        Ok(transactions)
    }

    async fn poll_tx(&mut self, signature: String) -> Result<Self::Transaction, anyhow::Error> {
        let transaction = self
            .client
            .get_transaction_by_signature(Signature::from_str(&signature).unwrap())
            .await?;

        Ok(transaction)
    }
}

impl<STR: SolanaStreamClientTrait> TransactionListener for SolanaListener<STR> {
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

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    use dotenv::dotenv;
    use relayer_base::config::config_from_yaml;
    use solana_sdk::commitment_config::CommitmentConfig;
    use testcontainers::{runners::AsyncRunner, ContainerAsync};
    use testcontainers_modules::postgres;

    use crate::{
        client::SolanaRpcClient, config::SolanaConfig, models::solana_subscriber_cursor::PostgresDB,
    };

    use super::*;

    async fn setup_test_container() -> (PostgresDB, ContainerAsync<postgres::Postgres>) {
        dotenv().ok();
        let container = postgres::Postgres::default()
            .with_init_sql(
                include_str!("../../migrations/0015_solana_subscriber_cursors.sql")
                    .to_string()
                    .into_bytes(),
            )
            .start()
            .await
            .unwrap();
        let connection_string = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await.unwrap(),
            container.get_host_port_ipv4(5432).await.unwrap()
        );
        let model = PostgresDB::new(&connection_string).await.unwrap();
        (model, container)
    }

    #[tokio::test]
    async fn test_poll_account() {
        let (postgres_cursor, _container) = setup_test_container().await;
        let network = std::env::var("NETWORK").expect("NETWORK must be set");
        let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();
        let solana_client: SolanaRpcClient =
            SolanaRpcClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3).unwrap();
        let mut solana_subscriber = SolanaPoller::new(solana_client, None).await.unwrap();

        let transactions = solana_subscriber
            .poll_account(Pubkey::from_str("9EHADvhP1vnYsk1XVYjJ4qpZ9jP33nHy84wo2CGnDDij").unwrap())
            .await
            .unwrap();

        println!("{:?}", transactions);
        assert_eq!(transactions.len(), 18);

        let transactions = solana_subscriber
            .poll_account(Pubkey::from_str("9EHADvhP1vnYsk1XVYjJ4qpZ9jP33nHy84wo2CGnDDij").unwrap())
            .await
            .unwrap();

        println!(
            "Second time it should be empty becasue cursor is updated and no other tx happened"
        );
        println!("{:?}", transactions);
        assert_eq!(transactions.len(), 0);
    }
}
