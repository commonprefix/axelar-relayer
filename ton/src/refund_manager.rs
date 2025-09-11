use async_trait::async_trait;
use relayer_base::utils::ThreadSafe;
use relayer_base::{
    error::RefundManagerError, gmp_api::gmp_types::RefundTask, includer::RefundManager,
};

#[derive(Clone)]
pub struct TONRefundManager;

impl TONRefundManager {
    pub fn new() -> Result<Self, RefundManagerError> {
        Ok(Self {})
    }
}

pub struct TONWallet;

#[async_trait]
impl RefundManager for TONRefundManager
where
    TONWallet: ThreadSafe,
{
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

    async fn get_wallet_lock(&self) -> Result<Self::Wallet, RefundManagerError> {
        Ok(TONWallet)
    }

    async fn release_wallet_lock(&self, _wallet: Self::Wallet) -> Result<(), RefundManagerError> {
        Ok(())
    }
}
