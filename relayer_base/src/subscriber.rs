use crate::queue::{Queue, QueueItem};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc};
use tracing::{debug, error, info};
use xrpl_api::Transaction;
use xrpl_types::AccountId;

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

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction;

    fn poll_account(
        &mut self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<Self::Transaction>, anyhow::Error>>;

    fn poll_tx(
        &mut self,
        tx_hash: String,
    ) -> impl Future<Output = Result<Self::Transaction, anyhow::Error>>;
}

pub struct Subscriber<TP: TransactionPoller> {
    transaction_poller: TP,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ChainTransaction {
    Xrpl(Transaction),
}

impl<TP: TransactionPoller> Subscriber<TP> {
    pub fn new(transaction_poller: TP) -> Self {
        Self { transaction_poller }
    }

    async fn work(&mut self, account: String, queue: Arc<Queue>) {
        // no match, just call poll_account
        let res = self
            .transaction_poller
            .poll_account(AccountId::from_address(&account).unwrap())
            .await;
        match res {
            Ok(txs) => {
                for tx in txs {
                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(chain_transaction.clone());
                    info!("Publishing transaction: {:?}", chain_transaction);
                    queue.publish(item.clone()).await;
                    debug!("Published tx: {:?}", item);
                }
            }
            Err(e) => {
                error!("Error getting txs: {:?}", e);
                debug!("Retrying in 2 seconds");
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
        for tx in txs {
            let res = self.transaction_poller.poll_tx(tx).await;
            match res {
                Ok(tx) => {
                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(chain_transaction.clone());
                    info!("Publishing transaction: {:?}", chain_transaction);
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
