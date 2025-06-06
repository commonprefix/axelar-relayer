use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer};
use std::sync::Arc;
use tokio::select;
use tracing::{debug, error, info, warn};

use crate::{
    config::Config,
    database::Database,
    error::IngestorError,
    gmp_api::{
        gmp_types::{BroadcastRequest, Task},
        GmpApi,
    },
    models::Models,
    payload_cache::PayloadCache,
    price_view::PriceView,
    queue::{Queue, QueueItem},
    subscriber::ChainTransaction,
    xrpl::XrplIngestor,
};

pub struct Ingestor<DB: Database> {
    gmp_api: Arc<GmpApi>,
    xrpl_ingestor: XrplIngestor<DB>,
}

impl<DB: Database> Ingestor<DB> {
    pub fn new(
        gmp_api: Arc<GmpApi>,
        config: Config,
        price_view: PriceView<DB>,
        payload_cache: PayloadCache<DB>,
        db_models: Models,
    ) -> Self {
        let xrpl_ingestor = XrplIngestor::new(
            gmp_api.clone(),
            config.clone(),
            price_view,
            payload_cache,
            db_models,
        );
        Self {
            gmp_api,
            xrpl_ingestor,
        }
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
        let events = match transaction {
            ChainTransaction::Xrpl(tx) => self.xrpl_ingestor.handle_transaction(tx).await?,
        };

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
                self.xrpl_ingestor.handle_verify(verify_task).await
            }
            Task::ReactToWasmEvent(react_to_wasm_event_task) => {
                info!("Consuming task: {:?}", react_to_wasm_event_task);
                self.xrpl_ingestor
                    .handle_wasm_event(react_to_wasm_event_task)
                    .await
            }
            Task::ConstructProof(construct_proof_task) => {
                info!("Consuming task: {:?}", construct_proof_task);
                self.xrpl_ingestor
                    .handle_construct_proof(construct_proof_task)
                    .await
            }
            Task::ReactToRetriablePoll(react_to_retriable_poll_task) => {
                info!("Consuming task: {:?}", react_to_retriable_poll_task);
                self.handle_retriable_task(
                    react_to_retriable_poll_task.task.request_payload,
                    react_to_retriable_poll_task.task.invoked_contract_address,
                )
                .await
            }
            Task::ReactToExpiredSigningSession(react_to_expired_signing_session_task) => {
                info!(
                    "Consuming task: {:?}",
                    react_to_expired_signing_session_task
                );
                self.handle_retriable_task(
                    react_to_expired_signing_session_task.task.request_payload,
                    react_to_expired_signing_session_task
                        .task
                        .invoked_contract_address,
                )
                .await
            }
            _ => Err(IngestorError::IrrelevantTask),
        }
    }

    async fn handle_retriable_task(
        &self,
        request_payload: String,
        invoked_contract_address: String,
    ) -> Result<(), IngestorError> {
        info!("Retrying: {:?}", request_payload);

        let payload: BroadcastRequest = BroadcastRequest::Generic(
            serde_json::from_str(&request_payload)
                .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?,
        );

        let request = self
            .gmp_api
            .post_broadcast(invoked_contract_address, &payload)
            .await
            .map_err(|e| IngestorError::PostEventError(e.to_string()))?;

        info!("Broadcast request sent: {:?}", request);

        Ok(())
    }
}
