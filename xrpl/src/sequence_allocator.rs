use async_trait::async_trait;
use relayer_base::{database::Database, error::IncluderError, includer::SequenceAllocator};
use std::sync::Arc;
use xrpl_types::AccountId;

use crate::client::XRPLClient;

pub struct XRPLSequenceAllocator<DB: Database> {
    db: DB,
    client: Arc<XRPLClient>,
}

impl<DB: Database> XRPLSequenceAllocator<DB> {
    pub fn new(db: DB, client: Arc<XRPLClient>) -> Self {
        Self { db, client }
    }
}

#[async_trait]
impl<DB: Database + Send + Sync> SequenceAllocator<DB> for XRPLSequenceAllocator<DB> {
    async fn get_next_sequence(&self, account: &AccountId) -> Result<u32, IncluderError> {
        // For now, return a simple implementation that gets the account sequence from XRPL
        // TODO: Implement the full database logic with FOR UPDATE once trait Send issues are resolved
        let account_info_request = xrpl_api::AccountInfoRequest {
            account: account.to_address(),
            ..Default::default()
        };

        let account_info = self
            .client
            .call(account_info_request)
            .await
            .map_err(|e| IncluderError::GenericError(e.to_string()))?;

        Ok(account_info.account_data.sequence)
    }
}
