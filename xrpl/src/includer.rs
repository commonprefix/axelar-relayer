use std::sync::Arc;

use relayer_base::{
    config::Config, database::Database, error::BroadcasterError, gmp_api::GmpApi,
    includer::Includer, payload_cache::PayloadCache, queue::Queue,
};

use crate::sequence_allocator::XRPLSequenceAllocator;

use super::{broadcaster::XRPLBroadcaster, client::XRPLClient, refund_manager::XRPLRefundManager};

use error_stack;
use r2d2;

pub struct XrplIncluder {}

impl XrplIncluder {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new<DB: Database + Clone + Send + Sync>(
        config: Config,
        gmp_api: Arc<GmpApi>,
        redis_pool: r2d2::Pool<redis::Client>,
        payload_cache: PayloadCache<DB>,
        construct_proof_queue: Arc<Queue>,
        db: DB,
    ) -> error_stack::Result<
        Includer<
            XRPLBroadcaster<DB>,
            Arc<XRPLClient>,
            XRPLRefundManager,
            DB,
            XRPLSequenceAllocator<DB>,
        >,
        BroadcasterError,
    > {
        let client =
            Arc::new(XRPLClient::new(config.xrpl_rpc.as_str(), 3).map_err(|e| {
                error_stack::report!(BroadcasterError::GenericError(e.to_string()))
            })?);

        let broadcaster = XRPLBroadcaster::new(Arc::clone(&client), db.clone())
            .map_err(|e| e.attach_printable("Failed to create XRPLBroadcaster"))?;

        let refund_manager =
            XRPLRefundManager::new(Arc::clone(&client), config, redis_pool.clone())
                .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?;

        let sequence_allocator = XRPLSequenceAllocator::new(db.clone(), Arc::clone(&client));

        let includer = Includer {
            chain_client: client,
            broadcaster,
            refund_manager,
            gmp_api,
            payload_cache,
            construct_proof_queue,
            redis_pool: redis_pool.clone(),
            sequence_allocator,
        };

        Ok(includer)
    }
}
