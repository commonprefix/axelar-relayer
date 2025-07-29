use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer};
use std::{future::Future, sync::Arc};
use tokio::select;
use tracing::{debug, error, info, warn};

use crate::{
    error::IngestorError,
    gmp_api::{
        gmp_types::{ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, Task, VerifyTask},
    },
    models::task_retries::PgTaskRetriesModel,
    queue::{Queue, QueueItem},
    subscriber::ChainTransaction,
};
use crate::gmp_api::GmpApiTrait;
use crate::utils::ThreadSafe;

pub struct Ingestor<I: IngestorTrait, G: GmpApiTrait + ThreadSafe> {
    gmp_api: Arc<G>,
    ingestor: I,
}

pub struct IngestorModels {
    pub task_retries: PgTaskRetriesModel,
}

pub trait IngestorTrait {
    fn handle_verify(&self, task: VerifyTask) -> impl Future<Output = Result<(), IngestorError>>;
    fn handle_transaction(
        &self,
        transaction: ChainTransaction,
    ) -> impl Future<Output = Result<Vec<Event>, IngestorError>>;
    fn handle_wasm_event(
        &self,
        task: ReactToWasmEventTask,
    ) -> impl Future<Output = Result<(), IngestorError>>;
    fn handle_construct_proof(
        &self,
        task: ConstructProofTask,
    ) -> impl Future<Output = Result<(), IngestorError>>;
    fn handle_retriable_task(
        &self,
        task: RetryTask,
    ) -> impl Future<Output = Result<(), IngestorError>>;
}

impl<I, G> Ingestor<I, G>
where
    I: IngestorTrait,
    G: GmpApiTrait + ThreadSafe
{
    pub fn new(gmp_api: Arc<G>, ingestor: I) -> Self {
        Self { gmp_api, ingestor }
    }

    async fn work(&self, consumer: &mut Consumer, queue: Arc<Queue>) {
        loop {
            info!("Waiting for messages from {}..", consumer.queue());
            match consumer.next().await {
                Some(Ok(delivery)) => {
                    let data = delivery.data.clone();
                    if let Err(e) = self.process_delivery(&data).await {
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

                        if let Err(nack_err) = queue.republish(delivery, force_requeue).await {
                            error!("Failed to republish message: {:?}", nack_err);
                        }
                    } else if let Err(ack_err) = delivery.ack(BasicAckOptions::default()).await {
                        let item = serde_json::from_slice::<QueueItem>(&delivery.data);
                        error!("Failed to ack item {:?}: {:?}", item, ack_err);
                    }
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

    pub async fn run(&self, events_queue: Arc<Queue>, tasks_queue: Arc<Queue>) {
        let mut events_consumer = events_queue.consumer().await.unwrap();
        let mut tasks_consumer = tasks_queue.consumer().await.unwrap();

        info!("Ingestor is alive.");

        select! {
            _ = self.work(&mut events_consumer, events_queue.clone()) => {
                warn!("Events consumer ended");
            },
            _ = self.work(&mut tasks_consumer, tasks_queue.clone()) => {
                warn!("Tasks consumer ended");
            }
        };
    }

    async fn process_delivery(&self, data: &[u8]) -> Result<(), IngestorError> {
        let item = serde_json::from_slice::<QueueItem>(data)
            .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?;

        self.consume(item).await
    }

    pub async fn consume(&self, item: QueueItem) -> Result<(), IngestorError> {
        match item {
            QueueItem::Task(task) => self.consume_task(task).await,
            QueueItem::Transaction(chain_transaction) => {
                self.consume_transaction(chain_transaction).await
            }
            _ => Err(IngestorError::IrrelevantTask),
        }
    }

    pub async fn consume_transaction(
        &self,
        transaction: ChainTransaction,
    ) -> Result<(), IngestorError> {
        info!("Consuming transaction: {:?}", transaction);
        let events = self.ingestor.handle_transaction(transaction).await?;

        if events.is_empty() {
            info!("No GMP events to post.");
            return Ok(());
        }

        info!("Posting events: {:?}", events.clone());
        let response = self
            .gmp_api
            .post_events(events)
            .await
            .map_err(|e| IngestorError::PostEventError(e.to_string()))?;

        for event_response in response {
            if event_response.status != "ACCEPTED" {
                error!("Posting event failed: {:?}", event_response.error.clone());
                if event_response.retriable.is_some() && event_response.retriable.unwrap() {
                    return Err(IngestorError::RetriableError(
                        // TODO: retry? Handle error responses for part of the batch
                        // Question: what happens if we send the same event multiple times?
                        event_response.error.clone().unwrap_or_default(),
                    ));
                }
            }
        }
        Ok(()) // TODO: better error handling
    }

    pub async fn consume_task(&self, task: Task) -> Result<(), IngestorError> {
        match task {
            Task::Verify(verify_task) => {
                info!("Consuming task: {:?}", verify_task);
                self.ingestor.handle_verify(verify_task).await
            }
            Task::ReactToWasmEvent(react_to_wasm_event_task) => {
                info!("Consuming task: {:?}", react_to_wasm_event_task);
                self.ingestor
                    .handle_wasm_event(react_to_wasm_event_task)
                    .await
            }
            Task::ConstructProof(construct_proof_task) => {
                info!("Consuming task: {:?}", construct_proof_task);
                self.ingestor
                    .handle_construct_proof(construct_proof_task)
                    .await
            }
            Task::ReactToRetriablePoll(react_to_retriable_poll_task) => {
                info!("Consuming task: {:?}", react_to_retriable_poll_task);
                self.ingestor
                    .handle_retriable_task(RetryTask::ReactToRetriablePoll(
                        react_to_retriable_poll_task,
                    ))
                    .await
            }
            Task::ReactToExpiredSigningSession(react_to_expired_signing_session_task) => {
                info!(
                    "Consuming task: {:?}",
                    react_to_expired_signing_session_task
                );
                self.ingestor
                    .handle_retriable_task(RetryTask::ReactToExpiredSigningSession(
                        react_to_expired_signing_session_task,
                    ))
                    .await
            }
            _ => Err(IngestorError::IrrelevantTask),
        }
    }
}
