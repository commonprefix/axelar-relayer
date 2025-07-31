use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer, message::Delivery};
use std::{future::Future, sync::Arc, collections::HashMap};
use std::collections::BTreeMap;
use lapin::types::{AMQPValue, ShortString};
use tokio::select;
use tracing::{debug, error, info, warn};
use opentelemetry::{global, Context, KeyValue};
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::{Span, TraceContextExt, Tracer};

use crate::{
    error::IngestorError,
    gmp_api::{
        gmp_types::{ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, Task, VerifyTask},
        GmpApi,
    },
    models::task_retries::PgTaskRetriesModel,
    queue::{Queue, QueueItem},
    subscriber::ChainTransaction,
    logging::deserialize_span_context,
};

pub struct Ingestor<I: IngestorTrait> {
    gmp_api: Arc<GmpApi>,
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

struct HeadersMap<'a>(&'a mut HashMap<String, String>);

impl Extractor for HeadersMap<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|metadata| Option::from(metadata.as_str()))
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
    }
}


impl<I: IngestorTrait> Ingestor<I> {
    pub fn new(gmp_api: Arc<GmpApi>, ingestor: I) -> Self {
        Self { gmp_api, ingestor }
    }

    async fn work(&self, consumer: &mut Consumer, queue: Arc<Queue>) {
        loop {
            info!("Waiting for messages from {}..", consumer.queue());
            match consumer.next().await {
                Some(Ok(delivery)) => {
                    // Extract headers from the delivery
                    let mut headers_map = HashMap::new();
                    if let Some(headers) = delivery.properties.headers() {
                        for (key, value) in headers.inner().iter() {
                            if let Some(value_str) = value.as_long_string() {
                                headers_map.insert(key.to_string(), value_str.to_string());
                            }
                        }
                    }

                    let data = delivery.data.clone();
                    if let Err(e) = self.process_delivery_with_headers(&data, headers_map).await {
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

    async fn process_delivery_with_headers(&self, data: &[u8], headers: HashMap<String, String>) -> Result<(), IngestorError> {
        let item = serde_json::from_slice::<QueueItem>(data)
            .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?;

        self.consume_with_headers(item, headers).await
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
        };
    }

    async fn process_delivery(&self, data: &[u8]) -> Result<(), IngestorError> {
        let item = serde_json::from_slice::<QueueItem>(data)
            .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?;

        self.consume(item).await
    }

    pub async fn consume(&self, item: QueueItem) -> Result<(), IngestorError> {
        self.consume_with_headers(item, HashMap::new()).await
    }

    pub async fn consume_with_headers(&self, item: QueueItem, mut headers: HashMap<String, String>) -> Result<(), IngestorError> {
        let parent_cx =
            global::get_text_map_propagator(|prop| prop.extract(&HeadersMap(&mut headers)));
        let tracer = global::tracer("ingestor");
        let mut span = tracer.start_with_context("ingestor", &parent_cx);
        
        let result = match item {
            QueueItem::Task(task) => {
                span.set_attribute(KeyValue::new("task_type", format!("{:?}", task)));
                self.consume_task(*task).await
            }
            QueueItem::Transaction(chain_transaction) => {
                  span.set_attribute(KeyValue::new("transaction_type", format!("{:?}", chain_transaction)));
                self.consume_transaction(chain_transaction).await
            }
            _ => Err(IngestorError::IrrelevantTask),
        };

        span.end();

        result
    }

    pub async fn consume_transaction(
        &self,
        transaction: Box<ChainTransaction>,
    ) -> Result<(), IngestorError> {
        let events = self.ingestor.handle_transaction(*transaction).await?;

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
                error!("Posting event failed: {:?}", event_response.error);
                if let Some(true) = event_response.retriable {
                    return Err(IngestorError::RetriableError(
                        // TODO: retry? Handle error responses for part of the batch
                        // Question: what happens if we send the same event multiple times?
                        event_response.error.unwrap_or_default(),
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
