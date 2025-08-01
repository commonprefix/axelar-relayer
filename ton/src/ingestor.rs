use opentelemetry::{global, Context, KeyValue};
use opentelemetry::global::ObjectSafeSpan;
use opentelemetry::trace::{FutureExt, Tracer};
use crate::parser::TraceParserTrait;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, VerifyTask,
};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::subscriber::ChainTransaction;
use tracing::warn;

pub struct TONIngestor<TP: TraceParserTrait> {
    trace_parser: TP,
}

impl<TP: TraceParserTrait> TONIngestor<TP> {
    pub fn new(trace_parser: TP) -> Self {
        Self { trace_parser }
    }
}

impl<TP: TraceParserTrait> IngestorTrait for TONIngestor<TP> {
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        warn!("handle_verify: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_transaction(
        &self,
        trace: ChainTransaction,
    ) -> Result<Vec<Event>, IngestorError> {
        let tracer = global::tracer("ton_ingestor");
        let mut span = tracer.start_with_context("ingestor.consume_transaction", &Context::current());
        
        let ChainTransaction::TON(trace) = trace else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!(
                "{trace:?}"
            )));
        };

        span.set_attribute(KeyValue::new("chain_trace_id", trace.trace_id.clone()));

        let events = self
            .trace_parser
            .parse_trace(*trace)
            .with_current_context()
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        Ok(events)
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        warn!("handle_wasm_event: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        warn!("handle_construct_proof: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        warn!("handle_retriable_task: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::ingestor::TONIngestor;
    use crate::parser::MockTraceParserTrait;
    use relayer_base::error::IngestorError;
    use relayer_base::gmp_api::gmp_types::{
        CommonTaskFields, ConstructProofTask, ConstructProofTaskFields, GatewayV2Message,
        ReactToExpiredSigningSessionTask, ReactToExpiredSigningSessionTaskFields,
        ReactToWasmEventTask, ReactToWasmEventTaskFields, RetryTask, VerifyTask, VerifyTaskFields,
        WasmEvent,
    };
    use relayer_base::ingestor::IngestorTrait;

    #[tokio::test]
    async fn test_handle_retriable_task_unimplemented() {
        let mock_parser = MockTraceParserTrait::new();
        let ingestor = TONIngestor::new(mock_parser);
        let task: RetryTask =
            RetryTask::ReactToExpiredSigningSession(ReactToExpiredSigningSessionTask {
                common: CommonTaskFields {
                    id: "".to_string(),
                    chain: "".to_string(),
                    timestamp: "".to_string(),
                    r#type: "".to_string(),
                    meta: None,
                },
                task: ReactToExpiredSigningSessionTaskFields {
                    session_id: 0,
                    broadcast_id: "".to_string(),
                    invoked_contract_address: "".to_string(),
                    request_payload: "".to_string(),
                },
            });
        let result = ingestor.handle_retriable_task(task).await;

        assert!(
            matches!(result, Err(IngestorError::GenericError(msg)) if msg.contains("Still not implemented"))
        );
    }

    #[tokio::test]
    async fn test_handle_construct_proof_unimplemented() {
        let mock_parser = MockTraceParserTrait::new();
        let ingestor = TONIngestor::new(mock_parser);
        let result = ingestor
            .handle_construct_proof(ConstructProofTask {
                common: CommonTaskFields {
                    id: "".to_string(),
                    chain: "".to_string(),
                    timestamp: "".to_string(),
                    r#type: "".to_string(),
                    meta: None,
                },
                task: ConstructProofTaskFields {
                    message: gateway_v2_message(),
                    payload: "".to_string(),
                },
            })
            .await;

        assert!(
            matches!(result, Err(IngestorError::GenericError(msg)) if msg.contains("Still not implemented"))
        );
    }

    fn gateway_v2_message() -> GatewayV2Message {
        GatewayV2Message {
            message_id: "".to_string(),
            source_chain: "".to_string(),
            source_address: "".to_string(),
            destination_address: "".to_string(),
            payload_hash: "".to_string(),
        }
    }

    #[tokio::test]
    async fn test_handle_wasm_event_unimplemented() {
        let mock_parser = MockTraceParserTrait::new();
        let ingestor = TONIngestor::new(mock_parser);
        let result = ingestor
            .handle_wasm_event(ReactToWasmEventTask {
                common: CommonTaskFields {
                    id: "".to_string(),
                    chain: "".to_string(),
                    timestamp: "".to_string(),
                    r#type: "".to_string(),
                    meta: None,
                },
                task: ReactToWasmEventTaskFields {
                    event: WasmEvent {
                        attributes: vec![],
                        r#type: "".to_string(),
                    },
                    height: 0,
                },
            })
            .await;

        assert!(
            matches!(result, Err(IngestorError::GenericError(msg)) if msg.contains("Still not implemented"))
        );
    }

    #[tokio::test]
    async fn test_handle_verify_unimplemented() {
        let mock_parser = MockTraceParserTrait::new();
        let ingestor = TONIngestor::new(mock_parser);
        let result = ingestor
            .handle_verify(VerifyTask {
                common: CommonTaskFields {
                    id: "".to_string(),
                    chain: "".to_string(),
                    timestamp: "".to_string(),
                    r#type: "".to_string(),
                    meta: None,
                },
                task: VerifyTaskFields {
                    message: gateway_v2_message(),
                    payload: "".to_string(),
                },
            })
            .await;

        assert!(
            matches!(result, Err(IngestorError::GenericError(msg)) if msg.contains("Still not implemented"))
        );
    }
}
