use std::sync::Arc;

use relayer_base::{
    database::Database, error::BroadcasterError, gmp_api::GmpApi, includer::Includer,
    payload_cache::PayloadCache, queue::Queue,
};

use crate::{client::XRPLClientTrait, models::queued_transactions::QueuedTransactionsModel};

use super::{broadcaster::XRPLBroadcaster, refund_manager::XRPLRefundManager};

use super::config::XRPLConfig;
use error_stack;
use r2d2;

pub struct XrplIncluder {}

impl XrplIncluder {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new<X: XRPLClientTrait, DB: Database, QM: QueuedTransactionsModel>(
        config: XRPLConfig,
        gmp_api: Arc<GmpApi>,
        redis_pool: r2d2::Pool<redis::Client>,
        payload_cache: PayloadCache<DB>,
        construct_proof_queue: Arc<Queue>,
        queued_tx_model: QM,
        chain_client: Arc<X>,
    ) -> error_stack::Result<
        Includer<XRPLBroadcaster<QM, X>, Arc<X>, XRPLRefundManager<X>, DB>,
        BroadcasterError,
    > {
        let broadcaster = XRPLBroadcaster::new(Arc::clone(&chain_client), queued_tx_model)
            .map_err(|e| e.attach_printable("Failed to create XRPLBroadcaster"))?;

        let refund_manager =
            XRPLRefundManager::new(Arc::clone(&chain_client), config, redis_pool.clone())
                .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?;

        let includer = Includer {
            chain_client,
            broadcaster,
            refund_manager,
            gmp_api,
            payload_cache,
            construct_proof_queue,
            redis_pool: redis_pool.clone(),
        };

        Ok(includer)
    }
}
