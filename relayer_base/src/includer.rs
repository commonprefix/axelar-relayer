use async_trait::async_trait;
use lapin::message::Delivery;
use lapin::options::BasicAckOptions;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info};

use crate::gmp_api::gmp_types::{ExecuteTaskFields, RefundTaskFields};
use crate::gmp_api::GmpApiTrait;
use crate::includer_worker::{IncluderWorker, IncluderWorkerTrait};
use crate::queue_consumer::QueueConsumer;
use crate::utils::ThreadSafe;
use crate::{
    database::Database,
    error::{BroadcasterError, IncluderError, RefundManagerError},
    gmp_api::gmp_types::RefundTask,
    queue::{Queue, QueueItem},
};

#[async_trait]
pub trait RefundManager
where
    Self::Wallet: Send,
{
    type Wallet;
    fn is_refund_manager_managed(&self) -> bool;

    async fn build_refund_tx(
        &self,
        recipient: String,
        amount: String,
        refund_id: &str,
        wallet: &Self::Wallet,
    ) -> Result<Option<(String, String, String)>, RefundManagerError>;
    async fn is_refund_processed(
        &self,
        refund_task: &RefundTask,
        refund_id: &str,
    ) -> Result<bool, RefundManagerError>;
    async fn get_wallet_lock(&self) -> Result<Self::Wallet, RefundManagerError>;
    async fn release_wallet_lock(&self, wallet: Self::Wallet) -> Result<(), RefundManagerError>;
}

#[derive(PartialEq, Debug)]
pub struct BroadcastResult<T> {
    pub transaction: T,
    pub tx_hash: String,
    pub status: Result<(), BroadcasterError>,
    pub message_id: Option<String>,
    pub source_chain: Option<String>,
}

#[async_trait]
pub trait Broadcaster
where
    Self::Transaction: ThreadSafe,
{
    type Transaction;

    async fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError>;

    async fn broadcast_refund(&self, tx_blob: String) -> Result<String, BroadcasterError>;
    async fn broadcast_execute_message(
        &self,
        message: ExecuteTaskFields,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError>;
    async fn broadcast_refund_message(
        &self,
        refund_task: RefundTaskFields,
    ) -> Result<String, BroadcasterError>;
}

pub struct Includer<B, C, R, DB, G>
where
    C: ThreadSafe + Clone,
    B: Broadcaster + ThreadSafe + Clone,
    R: RefundManager + ThreadSafe + Clone,
    DB: Database + ThreadSafe + Clone,
    G: GmpApiTrait + ThreadSafe + Clone,
{
    worker: IncluderWorker<B, C, R, DB, G>,
}

#[async_trait]
impl<B, C, R, DB, G> QueueConsumer for Includer<B, C, R, DB, G>
where
    C: ThreadSafe + Clone,
    B: Broadcaster + ThreadSafe + Clone,
    R: RefundManager + ThreadSafe + Clone,
    DB: Database + ThreadSafe + Clone,
    G: GmpApiTrait + ThreadSafe + Clone,
{
    async fn on_delivery(&self, delivery: Delivery, queue: Arc<Queue>, tracker: &TaskTracker) {
        let worker = self.worker.clone();
        let queue_clone = Arc::clone(&queue);
        tracker.spawn(async move {
            debug!("Spawned new includer task");
            let data = delivery.data.clone();
            if let Err(e) = worker.process_delivery(&data).await {
                let mut force_requeue = false;
                match e {
                    IncluderError::IrrelevantTask => {
                        debug!("Skipping irrelevant task");
                        force_requeue = true;
                    }
                    _ => {
                        error!("Failed to consume delivery: {:?}", e);
                    }
                }

                if let Err(nack_err) = queue_clone.republish(delivery, force_requeue).await {
                    error!("Failed to republish message: {:?}", nack_err);
                }
            } else if let Err(ack_err) = delivery.ack(BasicAckOptions::default()).await {
                let item = serde_json::from_slice::<QueueItem>(&delivery.data);
                error!("Failed to ack item {:?}: {:?}", item, ack_err);
            }
            debug!("Includer task finished");
        });
    }
}

impl<B, C, R, DB, G> Includer<B, C, R, DB, G>
where
    C: ThreadSafe + Clone,
    B: Broadcaster + ThreadSafe + Clone,
    R: RefundManager + ThreadSafe + Clone,
    DB: Database + ThreadSafe + Clone,
    G: GmpApiTrait + ThreadSafe + Clone,
{
    pub fn new(worker: IncluderWorker<B, C, R, DB, G>) -> Self {
        Self { worker }
    }
    pub async fn run(&self, queue: Arc<Queue>, token: CancellationToken) {
        if let Ok(mut consumer) = queue.consumer().await {
            info!("Includer is alive.");
            self.work(&mut consumer, Arc::clone(&queue), token.clone())
                .await;
            info!("Includer is done.");
        } else {
            error!("Failed to create consumer");
        }
    }
}
