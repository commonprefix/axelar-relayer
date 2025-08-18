use crate::queue::{Queue, QueueItem};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc};
use tracing::{debug, error, info};

pub trait TransactionListener {
    type Transaction;
    type Account;

    fn subscribe(
        &mut self,
        account: Self::Account,
    ) -> impl Future<Output = Result<(), anyhow::Error>>;

    fn unsubscribe(
        &mut self,
        accounts: Self::Account,
    ) -> impl Future<Output = Result<(), anyhow::Error>>;

    fn transaction_stream(
        &mut self,
    ) -> impl Future<Output = Pin<Box<dyn Stream<Item = Self::Transaction> + '_>>>;
}

pub trait TransactionPoller {
    type Transaction;
    type Account;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction;

    fn poll_account(
        &mut self,
        account: Self::Account,
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
    Xrpl(Box<xrpl_api::Transaction>),
    TON(Box<ton_types::ton_types::Trace>),
    Solana(Box<solana_sdk::transaction::Transaction>),
}

impl<TP: TransactionPoller> Subscriber<TP>
where
    TP::Account: Clone,
{
    pub fn new(transaction_poller: TP) -> Self {
        Self { transaction_poller }
    }

    async fn work(&mut self, account: TP::Account, queue: Arc<Queue>) {
        // no match, just call poll_account
        let res = self.transaction_poller.poll_account(account).await;
        match res {
            Ok(txs) => {
                for tx in txs {
                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
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

    pub async fn run(&mut self, account: TP::Account, queue: Arc<Queue>) {
        loop {
            self.work(account.clone(), Arc::clone(&queue)).await;
        }
    }

    pub async fn recover_txs(&mut self, txs: Vec<String>, queue: Arc<Queue>) {
        for tx in txs {
            let res = self.transaction_poller.poll_tx(tx).await;
            match res {
                Ok(tx) => {
                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
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
