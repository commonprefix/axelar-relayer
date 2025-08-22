use crate::gmp_api::{GmpApi, GmpApiDbAuditDecorator};
use crate::ingestor_worker::{IngestorWorker, IngestorWorkerTrait};
use crate::models::gmp_events::PgGMPEvents;
use crate::models::gmp_tasks::PgGMPTasks;
use crate::queue_consumer::QueueConsumer;
use crate::utils::{setup_heartbeat, ThreadSafe};
use crate::{
    error::IngestorError,
    gmp_api::gmp_types::{ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, VerifyTask},
    queue::{Queue, QueueItem},
    subscriber::ChainTransaction,
};
use async_trait::async_trait;
use lapin::message::Delivery;
use lapin::options::BasicAckOptions;
use redis::aio::ConnectionManager;
use std::sync::Arc;
use tokio::select;
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

pub struct Ingestor<W: IngestorWorkerTrait + Clone + ThreadSafe> {
    worker: W,
}

#[async_trait]
#[cfg_attr(any(test), mockall::automock)]
pub trait IngestorTrait: ThreadSafe {
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError>;
    async fn handle_transaction(
        &self,
        transaction: ChainTransaction,
    ) -> Result<Vec<Event>, IngestorError>;
    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError>;
    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError>;
    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError>;
}

#[async_trait]
impl<W> QueueConsumer for Ingestor<W>
where
    W: IngestorWorkerTrait + Clone + ThreadSafe,
{
    async fn on_delivery(&self, delivery: Delivery, queue: Arc<Queue>, tracker: &TaskTracker) {
        let worker = self.worker.clone();
        let queue_clone = Arc::clone(&queue);
        tracker.spawn(async move {
            debug!("Spawned new ingestor task");
            let data = delivery.data.clone();
            if let Err(e) = worker.process_delivery(&data).await {
                let mut force_requeue = false;
                match e {
                    IngestorError::IrrelevantTask => {
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
            debug!("Ingestor task finished");
        });
    }
}

impl<W> Ingestor<W>
where
    W: IngestorWorkerTrait + Clone + ThreadSafe,
{
    pub fn new(worker: W) -> Self {
        Self { worker }
    }

    pub async fn run(
        &self,
        events_queue: Arc<Queue>,
        tasks_queue: Arc<Queue>,
        token: CancellationToken,
    ) {
        let mut events_consumer = match events_queue.consumer().await {
            Ok(consumer) => consumer,
            Err(e) => {
                error!("Failed to create events consumer: {:?}", e);
                return;
            }
        };
        let mut tasks_consumer = match tasks_queue.consumer().await {
            Ok(consumer) => consumer,
            Err(e) => {
                error!("Failed to create tasks consumer: {:?}", e);
                return;
            }
        };

        info!("Ingestor is alive.");

        select! {
            _ = self.work(&mut events_consumer, Arc::clone(&events_queue), token.clone()) => {
                warn!("Events consumer ended");
            },
            _ = self.work(&mut tasks_consumer, Arc::clone(&tasks_queue), token.clone()) => {
                warn!("Tasks consumer ended");
            }
        }
    }
}

pub async fn run_ingestor(
    tasks_queue: &Arc<Queue>,
    events_queue: &Arc<Queue>,
    gmp_api: Arc<GmpApiDbAuditDecorator<GmpApi, PgGMPTasks, PgGMPEvents>>,
    redis_conn: ConnectionManager,
    chain_ingestor: Arc<dyn IngestorTrait>,
) -> anyhow::Result<()> {
    let worker = IngestorWorker::new(gmp_api, Arc::clone(&chain_ingestor));
    let token = CancellationToken::new();
    let ingestor = Ingestor::new(worker);
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    setup_heartbeat(
        "heartbeat:ingestor".to_owned(),
        redis_conn,
        Some(token.clone()),
    );
    let sigint_cloned_token = token.clone();
    let sigterm_cloned_token = token.clone();
    let ingestor_cloned_token = token.clone();
    let handle = tokio::spawn({
        let events = Arc::clone(events_queue);
        let tasks = Arc::clone(tasks_queue);
        let token_clone = token.clone();
        async move { ingestor.run(events, tasks, token_clone).await }
    });

    tokio::pin!(handle);

    tokio::select! {
        _ = sigint.recv()  => {
            sigint_cloned_token.cancel();
        },
        _ = sigterm.recv() => {
            sigterm_cloned_token.cancel();
        },
        _ = &mut handle => {
            info!("Ingestor stopped");
            ingestor_cloned_token.cancel();
        }
    }

    tasks_queue.close().await;
    events_queue.close().await;
    let _ = handle.await;
    Ok(())
}
