use crate::parser::TraceParser;
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, VerifyTask,
};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::price_view::PriceViewTrait;
use relayer_base::subscriber::ChainTransaction;
use tracing::warn;

pub struct TONIngestor<PV: PriceViewTrait> {
    trace_parser: TraceParser<PV>,
}

impl<PV: PriceViewTrait> TONIngestor<PV> {
    pub fn new(trace_parser: TraceParser<PV>) -> Self {
        Self { trace_parser }
    }
}

impl<PV: PriceViewTrait> IngestorTrait for TONIngestor<PV> {
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
        let ChainTransaction::TON(trace) = trace else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!(
                "{:?}",
                trace
            )));
        };

        let events = self
            .trace_parser
            .parse_trace(trace)
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
