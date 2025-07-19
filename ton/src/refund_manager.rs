use super::client::RestClient;
use crate::config::TONConfig;
use relayer_base::{
    error::RefundManagerError, gmp_api::gmp_types::RefundTask,
    includer::RefundManager,
};
use std::sync::Arc;

pub struct TONRefundManager {
    _client: Arc<dyn RestClient>,
    _redis_pool: r2d2::Pool<redis::Client>,
    _config: TONConfig,
}

impl TONRefundManager {
    pub fn new(
        client: Arc<dyn RestClient>,
        config: TONConfig,
        redis_pool: r2d2::Pool<redis::Client>,
    ) -> Result<Self, RefundManagerError> {
        Ok(Self {
            _client: client,
            _redis_pool: redis_pool,
            _config: config,
        })
    }
}

pub struct TONWallet;

impl RefundManager for TONRefundManager {
    type Wallet = TONWallet;

    fn is_refund_manager_managed(&self) -> bool {
        false
    }
    
    async fn build_refund_tx(
        &self,
        _recipient: String,
        _amount: String,
        _refund_id: &str,
        _wallet: &Self::Wallet,
    ) -> Result<Option<(String, String, String)>, RefundManagerError> {
        Ok(None)
    }

    async fn is_refund_processed(
        &self,
        _refund_task: &RefundTask,
        _refund_id: &str,
    ) -> Result<bool, RefundManagerError> {
        Ok(false)
    }

    fn get_wallet_lock(&self) -> Result<Self::Wallet, RefundManagerError> {
        Ok(TONWallet)
    }

    fn release_wallet_lock(&self, _wallet: Self::Wallet) -> Result<(), RefundManagerError> {
        Ok(())
    }
}
