use opentelemetry::{global, Context, KeyValue};
use opentelemetry::global::ObjectSafeSpan;
use opentelemetry::trace::{Tracer};
use crate::models::ton_trace::{EventSummary, UpdateEvents};
use crate::parser::TraceParserTrait;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, VerifyTask,
};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::models::gmp_events::EventModel;
use relayer_base::subscriber::ChainTransaction;
use tracing::{info, warn};

pub struct TONIngestor<TP: TraceParserTrait, TM: UpdateEvents + Send + Sync> {
    trace_parser: TP,
    ton_trace_model: TM,
}

impl<TP: TraceParserTrait, TM: UpdateEvents + Send + Sync> TONIngestor<TP, TM> {
    pub fn new(trace_parser: TP, ton_trace_model: TM) -> Self {
        Self {
            trace_parser,
            ton_trace_model,
        }
    }
}

impl<TP: TraceParserTrait, TM: UpdateEvents + Send + Sync> IngestorTrait for TONIngestor<TP, TM> {

    #[tracing::instrument(skip(self))]
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        warn!("handle_verify: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    #[tracing::instrument(skip(self))]
    async fn handle_transaction(
        &self,
        trace: ChainTransaction,
    ) -> Result<Vec<Event>, IngestorError> {
        let tracer = global::tracer("ton_ingestor");
        let mut span = tracer.start_with_context("ton_ingestor.consume_transaction", &Context::current());
        
        let ChainTransaction::TON(trace) = trace else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!(
                "{:?}",
                trace
            )));
        };

        let trace_id = trace.trace_id.clone();
        span.set_attribute(KeyValue::new("chain_trace_id", trace_id.clone()));

        let events = self
            .trace_parser
            .parse_trace(*trace)
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        // Map events to EventModels
        let event_models: Vec<EventModel> = events
            .iter()
            .map(|event| EventModel::from_event(event.clone()))
            .collect();

        // Create EventSummaries from EventModels
        let event_summaries: Vec<EventSummary> = event_models
            .iter()
            .map(|model| EventSummary {
                event_id: model.event_id.clone(),
                message_id: model.message_id.clone(),
                event_type: model.event_type.clone(),
            })
            .collect();

        info!("Created {} event summaries", event_summaries.len());

        // Update the trace with the event summaries
        if !event_summaries.is_empty() {
            self.ton_trace_model
                .update_events(trace_id, event_summaries)
                .await
                .map_err(|e| IngestorError::GenericError(e.to_string()))?;

            info!("Updated trace with event summaries");
        }

        Ok(events)
    }

    #[tracing::instrument(skip(self))]
    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        warn!("handle_wasm_event: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    #[tracing::instrument(skip(self))]
    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        warn!("handle_construct_proof: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    #[tracing::instrument(skip(self))]
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
    use crate::models::ton_trace::MockUpdateEvents;
    use crate::parser::MockTraceParserTrait;
    use relayer_base::error::IngestorError;
    use relayer_base::gmp_api::gmp_types::{
        Amount, CannotExecuteMessageReason, CommonEventFields, CommonTaskFields,
        ConstructProofTask, ConstructProofTaskFields, Event, EventMetadata, GatewayV2Message,
        ReactToExpiredSigningSessionTask, ReactToExpiredSigningSessionTaskFields,
        ReactToWasmEventTask, ReactToWasmEventTaskFields, RetryTask, VerifyTask, VerifyTaskFields,
        WasmEvent,
    };
    use relayer_base::ingestor::IngestorTrait;
    use relayer_base::subscriber::ChainTransaction;
    use ton_types::ton_types::Trace;

    #[tokio::test]
    async fn test_handle_retriable_task_unimplemented() {
        let mock_parser = MockTraceParserTrait::new();
        let mock_ton_trace_model = MockUpdateEvents::new();
        let ingestor = TONIngestor::new(mock_parser, mock_ton_trace_model);
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
        let mock_ton_trace_model = MockUpdateEvents::new();
        let ingestor = TONIngestor::new(mock_parser, mock_ton_trace_model);
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
        let mock_ton_trace_model = MockUpdateEvents::new();
        let ingestor = TONIngestor::new(mock_parser, mock_ton_trace_model);
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
        let mock_ton_trace_model = MockUpdateEvents::new();
        let ingestor = TONIngestor::new(mock_parser, mock_ton_trace_model);
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

    #[tokio::test]
    async fn test_handle_transaction() {
        let mut mock_parser = MockTraceParserTrait::new();

        // Create test events
        let event1 = Event::GasRefunded {
            common: CommonEventFields {
                r#type: "GAS_REFUNDED".to_string(),
                event_id: "event1".to_string(),
                meta: Some(EventMetadata {
                    tx_id: Some("tx1".to_string()),
                    from_address: None,
                    finalized: Some(true),
                    source_context: None,
                    timestamp: "2023-01-01T00:00:00Z".to_string(),
                }),
            },
            message_id: "message1".to_string(),
            recipient_address: "recipient1".to_string(),
            refunded_amount: Amount {
                token_id: None,
                amount: "100".to_string(),
            },
            cost: Amount {
                token_id: None,
                amount: "50".to_string(),
            },
        };

        let event2 = Event::CannotExecuteMessageV2 {
            common: CommonEventFields {
                r#type: "CANNOT_EXECUTE_MESSAGE_V2".to_string(),
                event_id: "event2".to_string(),
                meta: Some(EventMetadata {
                    tx_id: Some("tx2".to_string()),
                    from_address: None,
                    finalized: Some(true),
                    source_context: None,
                    timestamp: "2023-01-01T00:00:00Z".to_string(),
                }),
            },
            message_id: "message2".to_string(),
            source_chain: "source2".to_string(),
            reason: CannotExecuteMessageReason::InsufficientGas,
            details: "details2".to_string(),
        };

        let events = vec![event1, event2];

        // Setup mock parser to return our test events
        mock_parser.expect_parse_trace().returning(move |_| {
            let events = events.clone();
            Box::pin(async move { Ok(events) })
        });

        let mut mock_ton_trace_model = MockUpdateEvents::new();

        // Expect update_events to be called with the correct parameters
        mock_ton_trace_model
            .expect_update_events()
            .withf(|trace_id, event_summaries| {
                trace_id == "trace1"
                    && event_summaries.len() == 2
                    && event_summaries[0].event_id == "event1"
                    && event_summaries[0].message_id == Some("message1".to_string())
                    && event_summaries[0].event_type == "GAS_REFUNDED"
                    && event_summaries[1].event_id == "event2"
                    && event_summaries[1].message_id == Some("message2".to_string())
                    && event_summaries[1].event_type == "CANNOT_EXECUTE_MESSAGE_V2"
            })
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let ingestor = TONIngestor::new(mock_parser, mock_ton_trace_model);

        // Create a test trace
        let trace = Trace {
            trace_id: "trace1".to_string(),
            is_incomplete: false,
            start_lt: 100,
            end_lt: 200,
            transactions: vec![],
        };

        // Call handle_transaction
        let result = ingestor
            .handle_transaction(ChainTransaction::TON(Box::new(trace)))
            .await;

        // Verify the result
        assert!(result.is_ok());
        let returned_events = result.unwrap();
        assert_eq!(returned_events.len(), 2);

        // Verify first event
        match &returned_events[0] {
            Event::GasRefunded {
                common, message_id, ..
            } => {
                assert_eq!(common.event_id, "event1");
                assert_eq!(common.r#type, "GAS_REFUNDED");
                assert_eq!(message_id, "message1");
            }
            _ => panic!("Expected GasRefunded event"),
        }

        // Verify second event
        match &returned_events[1] {
            Event::CannotExecuteMessageV2 {
                common, message_id, ..
            } => {
                assert_eq!(common.event_id, "event2");
                assert_eq!(common.r#type, "CANNOT_EXECUTE_MESSAGE_V2");
                assert_eq!(message_id, "message2");
            }
            _ => panic!("Expected CannotExecuteMessageV2 event"),
        }
    }
}
