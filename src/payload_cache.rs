use redis::Commands;
use router_api::CrossChainId;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::gmp_api::gmp_types::GatewayV2Message;

const PAYLOAD_CACHE_EXPIRATION: u64 = 60 * 15; // 15 minutes
const PAYLOAD_CACHE_KEY_PREFIX: &str = "payload_cache";

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PayloadCacheValue {
    pub message: GatewayV2Message,
    pub payload: String,
}

pub struct PayloadCache {
    redis_pool: r2d2::Pool<redis::Client>,
}

impl PayloadCache {
    pub fn new(redis_pool: r2d2::Pool<redis::Client>) -> Self {
        Self { redis_pool }
    }

    fn key(&self, cc_id: CrossChainId) -> String {
        format!("{}:{}", PAYLOAD_CACHE_KEY_PREFIX, cc_id)
    }

    pub async fn get(
        &self,
        cc_id: CrossChainId,
    ) -> Result<Option<PayloadCacheValue>, anyhow::Error> {
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis connection: {}", e))?;

        let value: Option<String> = redis_conn
            .get(self.key(cc_id.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to get payload from Redis: {}", e))?;

        debug!("Got payload cache value for {}: {:?}", cc_id, value);

        if let Some(value) = value {
            let payload_cache_value: PayloadCacheValue = serde_json::from_str(&value)?;
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
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis connection: {}", e))?;

        let _: () = redis_conn
            .set_ex(self.key(cc_id), value, PAYLOAD_CACHE_EXPIRATION)
            .map_err(|e| anyhow::anyhow!("Failed to cache payload: {}", e))?;

        Ok(())
    }

    pub async fn clear(&self, cc_id: CrossChainId) -> Result<(), anyhow::Error> {
        debug!("Clearing payload cache for {}", cc_id);
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis connection: {}", e))?;

        let _: () = redis_conn
            .del(self.key(cc_id))
            .map_err(|e| anyhow::anyhow!("Failed to clear payload cache: {}", e))?;

        Ok(())
    }
}
