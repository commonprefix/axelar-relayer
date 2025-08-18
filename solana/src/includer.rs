use std::sync::Arc;

use relayer_base::{
    database::Database, error::BroadcasterError, gmp_api::GmpApiTrait, includer::Includer,
    payload_cache::PayloadCache, queue::Queue,
};

use crate::client::SolanaClientTrait;

use super::{broadcaster::SolanaBroadcaster, refund_manager::SolanaRefundManager};

use super::config::SolanaConfig;
use error_stack;
use redis::aio::ConnectionManager;
use relayer_base::utils::ThreadSafe;

pub struct SolanaIncluder {}

impl SolanaIncluder {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new<S: SolanaClientTrait, DB: Database, G: GmpApiTrait + ThreadSafe>(
        config: SolanaConfig,
        gmp_api: Arc<G>,
        redis_conn: ConnectionManager,
        payload_cache: PayloadCache<DB>,
        construct_proof_queue: Arc<Queue>,
        chain_client: Arc<S>,
    ) -> error_stack::Result<
        Includer<SolanaBroadcaster<S>, Arc<S>, SolanaRefundManager<S>, DB, G>,
        BroadcasterError,
    > {
        let broadcaster = SolanaBroadcaster::new(Arc::clone(&chain_client))
            .map_err(|e| e.attach_printable("Failed to create SolanaBroadcaster"))?;

        let refund_manager =
            SolanaRefundManager::new(Arc::clone(&chain_client), config, redis_conn.clone())
                .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?;

        let includer = Includer {
            chain_client,
            broadcaster,
            refund_manager,
            gmp_api,
            payload_cache,
            construct_proof_queue,
            redis_conn,
        };

        Ok(includer)
    }
}
