/*!

# TODO:
- source_context has a limit of 1000 characters, make sure we never exceed it
- Handle all errors, no unwraps() on reading from API
- Move handlers to a decorator pattern (?)
*/

use crate::event_mappers::{map_call_contract, map_gas_credit, map_message_approved, map_message_executed, map_native_gas_added};
use crate::parse_trace::{ParseTrace, ParsedTransaction, TraceTransactions};
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    ConstructProofTask, Event
    ,
    ReactToWasmEventTask, RetryTask, VerifyTask,
};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::subscriber::ChainTransaction;

#[derive(Default)]
pub struct TONIngestor {}

impl TONIngestor {
    pub fn new() -> Self {
        Self::default()
    }
}

type Mapping<'a> = (&'a [ParsedTransaction], fn(&ParsedTransaction) -> Event);


impl IngestorTrait for TONIngestor {
    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        println!("handle_verify: {:?}", task);

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

        let trace_transactions = TraceTransactions::from_trace(trace)
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let mut events = vec![];

        let mappings: Vec<Mapping> = vec![
            (&trace_transactions.message_approved, map_message_approved),
            (&trace_transactions.executed, map_message_executed),
            (&trace_transactions.gas_credit, map_gas_credit),
            (&trace_transactions.call_contract, map_call_contract),
            (&trace_transactions.gas_added, map_native_gas_added),
        ];

        for (txs, mapper) in mappings {
            events.extend(txs.iter().map(mapper));
        }

        Ok(events)
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        println!("handle_wasm_event: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        println!("handle_construct_proof: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        println!("handle_retriable_task: {:?}", task);

        Err(IngestorError::GenericError(
            "Still not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {}
