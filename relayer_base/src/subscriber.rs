use crate::queue::{Queue, QueueItem};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc};
use tracing::{debug, error, info, info_span, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

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

    fn transaction_id(&self, tx: &Self::Transaction) -> Option<String>;

    fn account_id(&self, account: &Self::Account) -> Option<String>;

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
}

impl<TP: TransactionPoller> Subscriber<TP>
where
    TP::Account: Clone,
{
    pub fn new(transaction_poller: TP) -> Self {
        Self { transaction_poller }
    }

    async fn work(&mut self, account: TP::Account, queue: Arc<Queue>)
    where
        <TP as TransactionPoller>::Transaction: 'static,
        <TP as TransactionPoller>::Account: 'static,
    {
        let res = self.transaction_poller.poll_account(account.clone()).await;
        match res {
            Ok(txs) => {
                for tx in txs {
                    let span = info_span!("chain_transaction");
                    if let Some(s) = self.transaction_poller.account_id(&account) {
                        span.set_attribute("chain_account_id", s);
                    }
                    if let Some(s) = self.transaction_poller.transaction_id(&tx) {
                        span.set_attribute("chain_transaction_id", s);
                    }

                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                    info!("Publishing transaction: {:?}", chain_transaction);

                    queue.publish(item.clone()).instrument(span).await;
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

    pub async fn run(&mut self, account: TP::Account, queue: Arc<Queue>)
    where
        <TP as TransactionPoller>::Transaction: 'static,
        <TP as TransactionPoller>::Account: 'static,
    {
        loop {
            self.work(account.clone(), Arc::clone(&queue)).await;
        }
    }

    pub async fn recover_txs(&mut self, txs: Vec<String>, queue: Arc<Queue>) {
        let span = info_span!("recover_txs");

        for tx in txs {
            let res = self
                .transaction_poller
                .poll_tx(tx)
                .instrument(span.clone())
                .await;

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
