use redis::AsyncCommands;
use std::sync::Arc;

use redis::aio::ConnectionManager;
use redis::{ExistenceCheck, SetExpiry, SetOptions};
use relayer_base::{
    error::RefundManagerError,
    gmp_api::gmp_types::RefundTask,
    includer::RefundManager,
    utils::{extract_and_decode_memo, parse_message_from_context},
};
use tracing::debug;

pub struct SolanaWallet;

use super::client::SolanaRpcClientTrait;
use super::config::SolanaConfig;

pub struct SolanaRefundManager<S: SolanaRpcClientTrait> {
    client: Arc<S>,
    redis_conn: ConnectionManager,
    config: SolanaConfig,
}

impl<S: SolanaRpcClientTrait> SolanaRefundManager<S> {
    pub fn new(
        client: Arc<S>,
        config: SolanaConfig,
        redis_conn: ConnectionManager,
    ) -> Result<Self, RefundManagerError> {
        Ok(Self {
            client,
            redis_conn,
            config,
        })
    }
}

impl<S: SolanaRpcClientTrait> RefundManager for SolanaRefundManager<S> {
    type Wallet = SolanaWallet;

    fn is_refund_manager_managed(&self) -> bool {
        true
    }

    async fn get_wallet_lock(&self) -> Result<Self::Wallet, RefundManagerError> {
        Err(RefundManagerError::GenericError(
            "Not implemented".to_string(),
        ))
    }

    async fn release_wallet_lock(&self, wallet: Self::Wallet) -> Result<(), RefundManagerError> {
        Err(RefundManagerError::GenericError(
            "Not implemented".to_string(),
        ))
    }

    async fn build_refund_tx(
        &self,
        recipient: String,
        drops: String,
        refund_id: &str,
        wallet: &Self::Wallet,
    ) -> Result<Option<(String, String, String)>, RefundManagerError> {
        Err(RefundManagerError::GenericError(
            "Not implemented".to_string(),
        ))
    }

    async fn is_refund_processed(
        &self,
        refund_task: &RefundTask,
        refund_id: &str,
    ) -> Result<bool, RefundManagerError> {
        Err(RefundManagerError::GenericError(
            "Not implemented".to_string(),
        ))
    }
}
