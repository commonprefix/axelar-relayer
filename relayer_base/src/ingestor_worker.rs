use crate::error::IngestorError;
use crate::gmp_api::gmp_types::{RetryTask, Task};
use crate::gmp_api::GmpApiTrait;
use crate::ingestor::IngestorTrait;
use crate::queue::QueueItem;
use crate::subscriber::ChainTransaction;
use crate::utils::ThreadSafe;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Clone)]
pub struct IngestorWorker<I: IngestorTrait + ThreadSafe, G: GmpApiTrait + ThreadSafe> {
    gmp_api: Arc<G>,
    ingestor: Arc<I>,
}

#[async_trait]
#[cfg_attr(any(test), mockall::automock)]
pub trait IngestorWorkerTrait {
    async fn process_delivery(&self, data: &[u8]) -> Result<(), IngestorError>;
}

impl<I, G> IngestorWorker<I, G>
where
    I: IngestorTrait + ThreadSafe,
    G: GmpApiTrait + ThreadSafe,
{
    pub fn new(gmp_api: Arc<G>, ingestor: Arc<I>) -> Self {
        Self { gmp_api, ingestor }
    }

    pub async fn consume(&self, item: QueueItem) -> Result<(), IngestorError> {
        match item {
            QueueItem::Task(task) => self.consume_task(*task).await,
            QueueItem::Transaction(chain_transaction) => {
                self.consume_transaction(chain_transaction).await
            }
            _ => Err(IngestorError::IrrelevantTask),
        }
    }

    pub async fn consume_transaction(
        &self,
        transaction: Box<ChainTransaction>,
    ) -> Result<(), IngestorError> {
        info!("Consuming transaction: {:?}", transaction);
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

#[async_trait]
impl<I, G> IngestorWorkerTrait for IngestorWorker<I, G>
where
    I: IngestorTrait + ThreadSafe,
    G: GmpApiTrait + ThreadSafe,
{
    async fn process_delivery(&self, data: &[u8]) -> Result<(), IngestorError> {
        let item = serde_json::from_slice::<QueueItem>(data)
            .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?;

        self.consume(item).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gmp_api::gmp_types::Task;
    use crate::gmp_api::MockGmpApi;
    use crate::ingestor::MockIngestorTrait;
    use crate::test_utils::fixtures;
    use mockall::predicate::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_process_delivery_success() {
        let mut mock_ingestor = MockIngestorTrait::new();
        let mock_gmp_api = MockGmpApi::new();

        let task = fixtures::verify_task();
        let verify_task = match task {
            Task::Verify(task) => task,
            _ => panic!("Expected VerifyTask"),
        };

        mock_ingestor
            .expect_handle_verify()
            .with(eq(verify_task.clone()))
            .returning(|_| Box::pin(async { Ok(()) }));

        let ingestor = IngestorWorker::new(Arc::new(mock_gmp_api), Arc::new(mock_ingestor));

        let queue_item = QueueItem::Task(Box::new(Task::Verify(verify_task)));
        let data = serde_json::to_vec(&queue_item).unwrap();

        let result = ingestor.process_delivery(&data).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_delivery_error() {
        let mut mock_ingestor = MockIngestorTrait::new();
        let mock_gmp_api = MockGmpApi::new();

        let task = fixtures::verify_task();
        let verify_task = match task {
            Task::Verify(task) => task,
            _ => panic!("Expected VerifyTask"),
        };

        mock_ingestor
            .expect_handle_verify()
            .with(eq(verify_task.clone()))
            .returning(|_| {
                Box::pin(async { Err(IngestorError::GenericError("Test error".to_string())) })
            });

        let ingestor = IngestorWorker::new(Arc::new(mock_gmp_api), Arc::new(mock_ingestor));

        let queue_item = QueueItem::Task(Box::new(Task::Verify(verify_task)));
        let data = serde_json::to_vec(&queue_item).unwrap();

        let result = ingestor.process_delivery(&data).await;

        assert!(result.is_err());
        match result {
            Err(IngestorError::GenericError(msg)) => {
                assert_eq!(msg, "Test error");
            }
            _ => panic!("Expected GenericError"),
        }
    }

    #[tokio::test]
    async fn test_process_delivery_irrelevant_task() {
        let mut mock_ingestor = MockIngestorTrait::new();
        let mock_gmp_api = MockGmpApi::new();

        let task = fixtures::verify_task();
        let verify_task = match task {
            Task::Verify(task) => task,
            _ => panic!("Expected VerifyTask"),
        };

        mock_ingestor
            .expect_handle_verify()
            .with(eq(verify_task.clone()))
            .returning(|_| Box::pin(async { Err(IngestorError::IrrelevantTask) }));

        let ingestor = IngestorWorker::new(Arc::new(mock_gmp_api), Arc::new(mock_ingestor));

        let queue_item = QueueItem::Task(Box::new(Task::Verify(verify_task)));
        let data = serde_json::to_vec(&queue_item).unwrap();

        let result = ingestor.process_delivery(&data).await;

        assert!(result.is_err());
        match result {
            Err(IngestorError::IrrelevantTask) => {}
            _ => panic!("Expected IrrelevantTask"),
        }
    }

    #[tokio::test]
    async fn test_process_delivery_invalid_json() {
        let mock_ingestor = MockIngestorTrait::new();
        let mock_gmp_api = MockGmpApi::new();

        let ingestor = IngestorWorker::new(Arc::new(mock_gmp_api), Arc::new(mock_ingestor));

        let data = b"{invalid json}";

        let result = ingestor.process_delivery(data).await;

        assert!(result.is_err());
        match result {
            Err(IngestorError::ParseError(_)) => {}
            _ => panic!("Expected ParseError"),
        }
    }
}
