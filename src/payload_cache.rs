use router_api::CrossChainId;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::database::Database;
use crate::gmp_api::gmp_types::GatewayV2Message;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PayloadCacheValue {
    pub message: GatewayV2Message,
    pub payload: String,
}

pub struct PayloadCache<DB: Database> {
    db: DB,
}

impl<DB: Database> PayloadCache<DB> {
    pub fn new(db: DB) -> Self {
        Self { db }
    }

    pub async fn get(
        &self,
        cc_id: CrossChainId,
    ) -> Result<Option<PayloadCacheValue>, anyhow::Error> {
        let value = self
            .db
            .get_payload(cc_id.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get payload from Redis: {}", e))?;

        debug!("Got payload for {}: {:?}", cc_id, value);

        if let Some(value) = value {
            let payload_cache_value: PayloadCacheValue = serde_json::from_str(value.as_str())?;
            Ok(Some(payload_cache_value))
        } else {
            Ok(None)
        }
    }

    pub async fn store(
        &self,
        cc_id: CrossChainId,
        value: PayloadCacheValue,
    ) -> Result<(), anyhow::Error> {
        let value = serde_json::to_string(&value)?;
        debug!("Storing payload cache value for {}: {}", cc_id, value);
        self.db
            .store_payload(cc_id, value)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to store payload: {}", e))?;

        Ok(())
    }

    pub async fn clear(&self, cc_id: CrossChainId) -> Result<(), anyhow::Error> {
        debug!("Clearing payload cache for {}", cc_id);
        self.db
            .clear_payload(cc_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to clear payload: {}", e))?;

        Ok(())
    }
}
