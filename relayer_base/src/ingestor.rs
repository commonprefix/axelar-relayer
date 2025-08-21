use crate::ingestor_worker::IngestorWorkerTrait;
use crate::utils::ThreadSafe;
use crate::{
    error::IngestorError,
    gmp_api::gmp_types::{ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, VerifyTask},
    queue::{Queue, QueueItem},
    subscriber::ChainTransaction,
};
use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer};
use std::{future::Future, sync::Arc};
use tokio::select;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

pub struct Ingestor<W: IngestorWorkerTrait + Clone + ThreadSafe> {
    worker: W,
    token: CancellationToken,
}

#[cfg_attr(any(test), mockall::automock)]
pub trait IngestorTrait: ThreadSafe {
    fn handle_verify(
        &self,
        task: VerifyTask,
    ) -> impl Future<Output = Result<(), IngestorError>> + Send;
    fn handle_transaction(
        &self,
        transaction: ChainTransaction,
    ) -> impl Future<Output = Result<Vec<Event>, IngestorError>> + Send;
    fn handle_wasm_event(
        &self,
        task: ReactToWasmEventTask,
    ) -> impl Future<Output = Result<(), IngestorError>> + Send;
    fn handle_construct_proof(
        &self,
        task: ConstructProofTask,
    ) -> impl Future<Output = Result<(), IngestorError>> + Send;
    fn handle_retriable_task(
        &self,
        task: RetryTask,
    ) -> impl Future<Output = Result<(), IngestorError>> + Send;
}

impl<W> Ingestor<W>
where
    W: IngestorWorkerTrait + Clone + ThreadSafe,
{
    pub fn new(worker: W, token: CancellationToken) -> Self {
        Self { worker, token }
    }

    async fn work(&self, consumer: &mut Consumer, queue: Arc<Queue>) {
        let tracker = TaskTracker::new();

        loop {
            debug!("Ingestor task tracker size: {}", tracker.len());
            info!("Waiting for messages from {}..", consumer.queue());
            select! {
                _ = self.token.cancelled() => {
                    info!("Cancellation requested; no longer awaiting consumer.next()");
                    break;
                }
                maybe_msg = consumer.next() => {
                    match maybe_msg {
                        Some(Ok(delivery)) => {
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

                                    if let Err(nack_err) =
                                        queue_clone.republish(delivery, force_requeue).await
                                    {
                                        error!("Failed to republish message: {:?}", nack_err);
                                    }
                                } else if let Err(ack_err) = delivery.ack(BasicAckOptions::default()).await
                                {
                                    let item = serde_json::from_slice::<QueueItem>(&delivery.data);
                                    error!("Failed to ack item {:?}: {:?}", item, ack_err);
                                }
                                debug!("Ingestor task finished");
                            });
                        }
                        Some(Err(e)) => {
                            error!("Failed to receive delivery: {:?}", e);
                        }
                        None => {
                            //TODO:  Consumer stream ended. Possibly handle reconnection logic here if needed.
                            warn!("No more messages from consumer.");
                        }
                    }
                }
            }
        }

        info!("Awaiting {} ingestor tasks", tracker.len());
        tracker.close();
        tracker.wait().await
    }

    pub async fn run(&self, events_queue: Arc<Queue>, tasks_queue: Arc<Queue>) {
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
            _ = self.work(&mut events_consumer, Arc::clone(&events_queue)) => {
                warn!("Events consumer ended");
            },
            _ = self.work(&mut tasks_consumer, Arc::clone(&tasks_queue)) => {
                warn!("Tasks consumer ended");
            }
        }
    }
}
