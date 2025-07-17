/*!

# TODO:
- source_context has a limit of 1000 characters, make sure we never exceed it
- Handle all errors, no unwraps() on reading from API
- Move handlers to a decorator pattern (?)
*/

use crate::event_mappers::{map_call_contract, map_gas_credit, map_message_approved, map_message_executed, map_message_native_gas_refunded, map_native_gas_added};
use crate::gas_calculator::GasCalculator;
use crate::parse_trace::{ParseTrace, TraceTransactions};
use relayer_base::error::IngestorError;
use relayer_base::gmp_api::gmp_types::{
    ConstructProofTask, Event, ReactToWasmEventTask, RetryTask, VerifyTask,
};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::subscriber::ChainTransaction;
use tracing::warn;

pub struct TONIngestor {
    gas_calculator: GasCalculator,
}

impl TONIngestor {
    pub fn new(gas_calculator: GasCalculator) -> Self {
        Self { gas_calculator }
    }
}

impl IngestorTrait for TONIngestor {
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

        let refund_gas_used = self
            .gas_calculator
            .calc_message_gas_native_gas_refunded(&trace.transactions)
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let total_gas_used = self
            .gas_calculator
            .calc_message_gas(&trace.transactions)
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let trace_transactions = TraceTransactions::from_trace(trace)
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;
        let mut events = vec![];

        // TODO: Document this: we assume only one executed message or more than one approved
        // message per trace. We should never have a combination. If we had a combination of
        // different transactions in the same trace (unlikely, because it's coming from us)
        // this approach won't work and we'll have to follow each subpath of trace
        let mut per_msg_gas: u64 = total_gas_used;

        if !trace_transactions.message_approved.is_empty() {
            per_msg_gas = total_gas_used / trace_transactions.message_approved.len() as u64;
        }

        for tx in &trace_transactions.message_approved {
            events.push(map_message_approved(tx, per_msg_gas));
        }

        for tx in &trace_transactions.executed {
            events.push(map_message_executed(tx, per_msg_gas));
        }

        for tx in &trace_transactions.gas_refunded {
            events.push(map_message_native_gas_refunded(tx, refund_gas_used));
        }

        for tx in &trace_transactions.gas_credit {
            events.push(map_gas_credit(tx));
        }

        for tx in &trace_transactions.call_contract {
            events.push(map_call_contract(tx));
        }

        for tx in &trace_transactions.gas_added {
            events.push(map_native_gas_added(tx));
        }

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
mod tests {}
