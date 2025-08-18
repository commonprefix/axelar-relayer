use std::sync::Arc;

use solana_sdk::transaction::Transaction;
use tracing::{debug, error, warn};

use relayer_base::gmp_api::gmp_types::{ExecuteTaskFields, RefundTaskFields};
use relayer_base::{
    error::BroadcasterError,
    includer::{BroadcastResult, Broadcaster},
};

use super::client::SolanaClientTrait;

pub struct SolanaBroadcaster<S: SolanaClientTrait> {
    client: Arc<S>,
}

impl<S: SolanaClientTrait> SolanaBroadcaster<S> {
    pub fn new(client: Arc<S>) -> error_stack::Result<Self, BroadcasterError> {
        Ok(SolanaBroadcaster { client })
    }
}

impl<S: SolanaClientTrait> Broadcaster for SolanaBroadcaster<S> {
    type Transaction = Transaction;

    async fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        Err(BroadcasterError::GenericError(
            "Not implemented".to_string(),
        ))
    }

    async fn broadcast_refund(&self, tx_blob: String) -> Result<String, BroadcasterError> {
        Err(BroadcasterError::GenericError(
            "Not implemented".to_string(),
        ))
    }

    async fn broadcast_execute_message(
        &self,
        message: ExecuteTaskFields,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        Err(BroadcasterError::GenericError(
            "Not implemented".to_string(),
        ))
    }

    async fn broadcast_refund_message(
        &self,
        refund_task: RefundTaskFields,
    ) -> Result<String, BroadcasterError> {
        Err(BroadcasterError::GenericError(
            "Not implemented".to_string(),
        ))
    }
}
