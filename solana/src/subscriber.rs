use std::{pin::Pin, str::FromStr};

use crate::{client::SolanaRpcClientTrait, models::solana_subscriber_cursor::SubscriberCursor};
use anyhow::anyhow;
use futures::Stream;
use relayer_base::{
    error::SubscriberError,
    subscriber::{ChainTransaction, TransactionListener, TransactionPoller},
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_types::solana_types::SolanaTransaction;
use tracing::error;

pub struct SolanaPoller<X: SolanaRpcClientTrait, SC: SubscriberCursor> {
    client: X,
    last_signature_checked: Option<Signature>,
    cursor_model: SC,
    context: String,
}

impl<X: SolanaRpcClientTrait, SC: SubscriberCursor> SolanaPoller<X, SC> {
    pub async fn new(
        client: X,
        context: String,
        cursor_model: SC,
    ) -> Result<Self, SubscriberError> {
        let maybe_last_signature_checked = cursor_model.get_latest_signature(&context).await;

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
                .store_latest_signature(&self.context, signature.to_string())
                .await
                .map_err(|e| anyhow!("Error storing latest signature: {:?}", e)),
            None => Err(anyhow!("No signature to store")),
        }
    }
}

impl<X: SolanaRpcClientTrait, SC: SubscriberCursor> TransactionPoller for SolanaPoller<X, SC> {
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

impl<X: SolanaRpcClientTrait, SC: SubscriberCursor> TransactionListener for SolanaPoller<X, SC> {
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Box::new(tx))
    }

    async fn subscribe(&mut self, account: Self::Account) -> Result<(), anyhow::Error> {
        // let (mut sub, _unsub) = self
        //     .client
        //     .logs_subscribe(
        //         // we subscribe to all txs that contain the pubkey
        //         RpcTransactionLogsFilter::Mentions(vec![
        //             "DaejccUfXqoAFTiDTxDuMQfQ9oa6crjtR9cT52v1AvGK".to_string(),
        //         ]),
        //         RpcTransactionLogsConfig {
        //             commitment: Some(CommitmentConfig::confirmed()),
        //         },
        //     )
        //     .await?;
        Err(anyhow!("Not implemented"))
    }

    async fn unsubscribe(&mut self, accounts: Self::Account) -> Result<(), anyhow::Error> {
        Err(anyhow!("Not implemented"))
    }

    async fn transaction_stream(&mut self) -> Pin<Box<dyn Stream<Item = Self::Transaction> + '_>> {
        todo!()
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
