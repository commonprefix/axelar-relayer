use std::{future::Future, str::FromStr, sync::Arc};

use crate::{
    client::{LogsSubscription, SolanaRpcClientTrait, SolanaStreamClientTrait},
    models::solana_subscriber_cursor::SubscriberCursor,
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
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_types::solana_types::SolanaTransaction;
use tracing::{debug, error, info, warn};

pub struct SolanaPoller<RPC: SolanaRpcClientTrait, SC: SubscriberCursor> {
    client: RPC,
    last_signature_checked: Option<Signature>,
    cursor_model: SC,
    context: String,
}

pub struct SolanaListener<STR: SolanaStreamClientTrait, SC: SubscriberCursor> {
    client: STR,
    last_signature_checked: Option<Signature>,
    cursor_model: SC,
    context: String,
}

pub trait TransactionListener {
    type Transaction;
    type Account;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction;

    fn subscriber(
        &mut self,
        account: Self::Account,
    ) -> impl Future<Output = Result<LogsSubscription<'_>, anyhow::Error>>;

    fn unsubscribe(&mut self) -> impl Future<Output = ()>;
}

impl<STR: SolanaStreamClientTrait, SC: SubscriberCursor> SolanaListener<STR, SC> {
    pub async fn new(
        client: STR,
        context: String,
        cursor_model: SC,
    ) -> Result<Self, SubscriberError> {
        let maybe_last_signature_checked = cursor_model.get_latest_signature(context.clone()).await;

        let last_signature_checked = match maybe_last_signature_checked {
            Ok(signature_option) => match signature_option {
                Some(signature_str) => {
                    let maybe_sig = match Signature::from_str(&signature_str) {
                        Ok(sig) => Some(sig),
                        Err(e) => {
                            error!("Error parsing signature: {:?}", e);
                            None
                        }
                    };
                    maybe_sig
                }
                None => None,
            },
            Err(e) => {
                error!("Error getting latest signature: {:?}", e);
                None
            }
        };

        Ok(SolanaListener {
            client,
            last_signature_checked,
            cursor_model,
            context,
        })
    }

    pub async fn run(&mut self, account: Pubkey, queue: Arc<Queue>, subscriber_cursor: Arc<SC>) {
        loop {
            let context = self.context.clone();
            let subscriber_stream_res = self.subscriber(account.clone()).await;
            if let Ok(mut subscriber_stream) = subscriber_stream_res {
                loop {
                    match Self::work(&mut subscriber_stream, Arc::clone(&queue)).await {
                        Ok(Some(signature)) => {
                            if let Err(e) = subscriber_cursor
                                .store_latest_signature(context.clone(), signature.to_string())
                                .await
                            {
                                error!("Error storing latest signature: {:?}", e);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            error!("Error processing stream: {:?}", e);
                        }
                    }
                }
            } else {
                error!(
                    "Error getting subscriber stream: {:?}",
                    subscriber_stream_res.err()
                );
                debug!("Retrying in 2 seconds");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }

    // TOOD: Create a general stream work function and seperate it from poll work
    async fn work(
        subscriber_stream: &mut LogsSubscription<'_>,
        queue: Arc<Queue>,
    ) -> Result<Option<Signature>, anyhow::Error> {
        info!("Waiting for messages from solana subscriber...");
        match subscriber_stream.next().await {
            Some(response) => {
                let tx = create_solana_tx_from_rpc_response(response)?;
                let chain_transaction = ChainTransaction::Solana(Box::new(tx.clone()));

                let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                info!("Publishing transaction: {:?}", chain_transaction);
                queue.publish(item.clone()).await;
                debug!("Published tx: {:?}", item);
                Ok(Some(tx.signature))
            }
            None => {
                warn!("Stream was closed");
                Err(anyhow!("Stream was closed"))
            }
        }
    }
}

impl<RPC: SolanaRpcClientTrait, SC: SubscriberCursor> SolanaPoller<RPC, SC> {
    pub async fn new(
        client: RPC,
        context: String,
        cursor_model: SC,
    ) -> Result<Self, SubscriberError> {
        let maybe_last_signature_checked = cursor_model.get_latest_signature(context.clone()).await;

        let last_signature_checked = match maybe_last_signature_checked {
            Ok(signature_option) => match signature_option {
                Some(signature_str) => {
                    let maybe_sig = match Signature::from_str(&signature_str) {
                        Ok(sig) => Some(sig),
                        Err(e) => {
                            error!("Error parsing signature: {:?}", e);
                            None
                        }
                    };
                    maybe_sig
                }
                None => None,
            },
            Err(e) => {
                error!("Error getting latest signature: {:?}", e);
                None
            }
        };

        Ok(SolanaPoller {
            client,
            last_signature_checked,
            cursor_model,
            context,
        })
    }

    pub async fn store_last_signature_checked(&mut self) -> Result<(), anyhow::Error> {
        match self.last_signature_checked {
            Some(signature) => self
                .cursor_model
                .store_latest_signature(self.context.clone(), signature.to_string())
                .await
                .map_err(|e| anyhow!("Error storing latest signature: {:?}", e)),
            None => Err(anyhow!("No signature to store")),
        }
    }
}

impl<RPC: SolanaRpcClientTrait, SC: SubscriberCursor> TransactionPoller for SolanaPoller<RPC, SC> {
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
            if let Err(err) = self.store_last_signature_checked().await {
                error!("{:?}", err);
                return Err(err);
            }
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

impl<STR: SolanaStreamClientTrait, SC: SubscriberCursor> TransactionListener
    for SolanaListener<STR, SC>
{
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Box::new(tx))
    }

    async fn subscriber(
        &mut self,
        account: Self::Account,
    ) -> Result<LogsSubscription<'_>, anyhow::Error> {
        self.client.logs_subscriber(account.to_string()).await
    }

    async fn unsubscribe(&mut self) {
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
        let mut solana_subscriber =
            SolanaPoller::new(solana_client, "default".to_string(), postgres_cursor)
                .await
                .unwrap();

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
