use futures::Stream;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc};
use tracing::{debug, error, info};
use xrpl_api::Transaction;
use xrpl_types::AccountId;

use crate::{
    database::Database,
    queue::{Queue, QueueItem},
    xrpl::XrplSubscriber,
};

pub trait TransactionListener {
    type Transaction;

    fn subscribe(&mut self, account: AccountId) -> impl Future<Output = Result<(), anyhow::Error>>;

    fn unsubscribe(
        &mut self,
        accounts: AccountId,
    ) -> impl Future<Output = Result<(), anyhow::Error>>;

    fn transaction_stream(
        &mut self,
    ) -> impl Future<Output = Pin<Box<dyn Stream<Item = Self::Transaction> + '_>>>;
}

pub trait TransactionPoller {
    type Transaction;

    fn poll_account(
        &mut self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Self::Transaction>, anyhow::Error>>;

    fn poll_tx(
        &mut self,
        tx_hash: String,
    ) -> impl Future<Output = Result<Self::Transaction, anyhow::Error>>;
}

pub enum Subscriber<DB: Database> {
    Xrpl(XrplSubscriber<DB>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ChainTransaction {
    Xrpl(Transaction),
}

impl<DB: Database> Subscriber<DB> {
    pub async fn new_xrpl(url: &str, postgres_db: DB) -> Subscriber<DB> {
        let client = XrplSubscriber::new(url, postgres_db).await.unwrap();
        Subscriber::Xrpl(client)
    }

    async fn work(&mut self, account: String, queue: Arc<Queue>) {
        match self {
            Subscriber::Xrpl(sub) => {
                let res = sub
                    .poll_account(AccountId::from_address(&account).unwrap())
                    .await;
                match res {
                    Ok(txs) => {
                        for tx in txs {
                            let chain_transaction = ChainTransaction::Xrpl(tx.clone());
                            let item = &QueueItem::Transaction(chain_transaction.clone());
                            info!("Publishing XRPL transaction: {:?}", tx.common().hash);
                            queue.publish(item.clone()).await;
                            debug!("Published tx: {:?}", item);
                        }
                    }
                    Err(e) => {
                        error!("Error getting txs: {:?}", e);
                        debug!("Retrying in 2 seconds");
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await
    }

    pub async fn run(&mut self, account: String, queue: Arc<Queue>) {
        loop {
            self.work(account.clone(), queue.clone()).await;
        }
    }

    pub async fn recover_txs(&mut self, txs: Vec<String>, queue: Arc<Queue>) {
        match self {
            Subscriber::Xrpl(sub) => {
                for tx in txs {
                    let res = sub.poll_tx(tx).await;
                    match res {
                        Ok(tx) => {
                            let chain_transaction = ChainTransaction::Xrpl(tx.clone());
                            let item = &QueueItem::Transaction(chain_transaction.clone());
                            info!("Publishing XRPL transaction: {:?}", tx.common().hash);
                            queue.publish(item.clone()).await;
                            debug!("Published tx: {:?}", item);
                        }
                        Err(e) => {
                            error!("Error getting txs: {:?}", e);
                            debug!("Retrying in 2 seconds");
                        }
                    }
                }
            }
        }
    }
}
