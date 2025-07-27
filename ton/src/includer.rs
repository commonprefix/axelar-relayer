use super::{broadcaster::TONBroadcaster, client::TONRpcClient, refund_manager::TONRefundManager};
use crate::client::RestClient;
use crate::config::TONConfig;
use crate::gas_estimator::TONGasEstimator;
use crate::high_load_query_id_db_wrapper::HighLoadQueryIdDbWrapper;
use crate::lock_manager::RedisLockManager;
use crate::wallet_manager::WalletManager;
use relayer_base::{
    database::Database, error::BroadcasterError, gmp_api::{GmpApi, GmpApiTrait}, includer::Includer,
    payload_cache::PayloadCache, queue::Queue,
};
use std::sync::Arc;
use tonlib_core::TonAddress;

pub struct TONIncluder {}

impl TONIncluder {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new<DB: Database, G: GmpApiTrait + Send + Sync + 'static>(
        config: TONConfig,
        gmp_api: Arc<G>,
        redis_pool: r2d2::Pool<redis::Client>,
        payload_cache_for_includer: PayloadCache<DB>,
        construct_proof_queue: Arc<Queue>,
        high_load_query_id_db_wrapper: Arc<HighLoadQueryIdDbWrapper>,
    ) -> error_stack::Result<
        Includer<TONBroadcaster<TONGasEstimator>, Arc<dyn RestClient>, TONRefundManager, DB, G>,
        BroadcasterError,
    > {
        let config_for_refund_manager = config.clone();
        let ton_rpc = config.ton_rpc;
        let ton_api_key = config.ton_api_key;
        let wallets = config.wallets;
        let ton_gateway = config.ton_gateway;
        let ton_gas_service = config.ton_gas_service;

        let lock_manager = Arc::new(RedisLockManager::new(redis_pool.clone()));
        let wallet_manager = Arc::new(WalletManager::new(wallets, lock_manager).await);

        let client: Arc<dyn RestClient> = Arc::new(
            TONRpcClient::new(ton_rpc, ton_api_key, 5, 5, 30)
                .await
                .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?,
        );

        let gateway_address = TonAddress::from_base64_url(ton_gateway.as_str())
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;
        let gas_service_address = TonAddress::from_base64_url(ton_gas_service.as_str())
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;

        let broadcaster = TONBroadcaster::new(
            Arc::clone(&wallet_manager),
            Arc::clone(&client),
            high_load_query_id_db_wrapper,
            gateway_address,
            gas_service_address,
            config.common_config.chain_name,
            TONGasEstimator::new(config.gas_estimates.clone()),
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
            payload_cache: payload_cache_for_includer,
            construct_proof_queue,
            redis_pool: redis_pool.clone(),
        };

        Ok(includer)
    }
}
