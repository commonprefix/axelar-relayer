use super::{broadcaster::TONBroadcaster, client::TONRpcClient, refund_manager::TONRefundManager};
use crate::client::RestClient;
use crate::config::TONConfig;
use crate::high_load_query_id_db_wrapper::HighLoadQueryIdDbWrapper;
use crate::lock_manager::RedisLockManager;
use crate::wallet_manager::WalletManager;
use relayer_base::{
    database::Database, error::BroadcasterError, gmp_api::GmpApi, includer::Includer,
    payload_cache::PayloadCache, queue::Queue,
};
use std::sync::Arc;
use tonlib_core::TonAddress;

pub struct TONIncluder {}

impl TONIncluder {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new<'a, DB: Database>(
        config: TONConfig,
        gmp_api: Arc<GmpApi>,
        redis_pool: r2d2::Pool<redis::Client>,
        payload_cache: PayloadCache<DB>,
        construct_proof_queue: Arc<Queue>,
        high_load_query_id_db_wrapper: Arc<HighLoadQueryIdDbWrapper>,
    ) -> error_stack::Result<
        Includer<TONBroadcaster, Arc<dyn RestClient>, TONRefundManager, DB>,
        BroadcasterError,
    > {
        let config_for_refund_manager = config.clone();
        let ton_rpc = config.ton_rpc;
        let ton_api_key = config.ton_api_key;
        let wallets = config.wallets;
        let ton_gateway = config.ton_gateway;

        let lock_manager = Arc::new(RedisLockManager::new(redis_pool.clone()));
        let wallet_manager = Arc::new(WalletManager::new(wallets, lock_manager).await);

        let client: Arc<dyn RestClient> = Arc::new(
            TONRpcClient::new(ton_rpc, 3, ton_api_key)
                .await
                .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?,
        );

        let gateway_address = TonAddress::from_base64_url(ton_gateway.as_str()).unwrap();
        let internal_message_value = 1_000_000_000u32; // TODO: Do not hardcode this

        let broadcaster = TONBroadcaster::new(
            Arc::clone(&wallet_manager),
            Arc::clone(&client),
            high_load_query_id_db_wrapper,
            gateway_address,
            internal_message_value,
            config.common_config.chain_name,
        )
        .map_err(|e| e.attach_printable("Failed to create TONBroadcaster"))?;

        let refund_manager = TONRefundManager::new(
            Arc::clone(&client),
            config_for_refund_manager,
            redis_pool.clone(),
        )
        .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?;

        let includer = Includer {
            chain_client: client,
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
