use crate::queue::{Queue, QueueItem};
use futures::Stream;
use lapin::BasicProperties;
use lapin::types::{AMQPValue, FieldTable, ShortString};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use opentelemetry::{global, Context, KeyValue};
use opentelemetry::propagation::Injector;
use opentelemetry::trace::{Span, TraceContextExt, Tracer};
use tracing::{debug, error, info, span, Level};
use crate::logging::serialize_span;

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
}

struct HeadersMap<'a>(&'a mut BTreeMap<ShortString, AMQPValue>);

impl Injector for HeadersMap<'_> {
    /// Set a key and value in the MetadataMap.  Does nothing if the key or value are not valid inputs
    fn set(&mut self, key: &str, value: String) {
        let Ok(key) = ShortString::from(key).try_into();
        let Ok(val) = AMQPValue::LongString(value.into()).try_into();
        self.0.insert(key, val);
    }
}


impl<TP: TransactionPoller> Subscriber<TP>
where
    TP::Account: Clone + ToString,
    TP::Transaction: ToString,
{
    pub fn new(transaction_poller: TP) -> Self {
        Self { transaction_poller }
    }

    async fn work(&mut self, account: TP::Account, queue: Arc<Queue>) {
        let tracer = global::tracer("tracer");
        let acc_name = account.to_string();

        let res = self.transaction_poller.poll_account(account.clone()).await;
        match res {
            Ok(txs) => {
                for tx in txs {
                    let mut span = tracer.start("subscriber.work");

                    span.set_attribute(KeyValue::new("chain_account", acc_name.to_string()));
                    span.set_attribute(KeyValue::new("chain_transaction_id", tx.to_string()));

                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                    info!("Publishing transaction: {:?}", chain_transaction);
                    let mut headers = BTreeMap::new();

                    let cx = Context::current_with_span(span);
                    global::get_text_map_propagator(|propagator| {
                        propagator.inject_context(&cx, &mut HeadersMap(&mut headers));
                    });

                    debug!("Sending headers: {:?}", headers);

                    let properties = BasicProperties::default()
                        .with_delivery_mode(2)
                        .with_headers(FieldTable::from(headers));

                    queue.publish_with_properties(item.clone(), properties).await;
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
        let tracer = global::tracer("tracer");

        for tx in txs {
            let res = self.transaction_poller.poll_tx(tx).await;
            match res {
                Ok(tx) => {
                    let span = tracer.start("subscriber.recover_txs");

                    let chain_transaction = self.transaction_poller.make_queue_item(tx);
                    let item = &QueueItem::Transaction(Box::new(chain_transaction.clone()));
                    info!("Publishing transaction: {:?}", chain_transaction);

                    let mut headers = BTreeMap::new();

                    let cx = Context::current_with_span(span);
                    global::get_text_map_propagator(|propagator| {
                        propagator.inject_context(&cx, &mut HeadersMap(&mut headers));
                    });

                    let properties = BasicProperties::default()
                        .with_delivery_mode(2)
                        .with_headers(FieldTable::from(headers));

                    queue.publish_with_properties(item.clone(), properties).await;
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
