use redis::Commands;
use router_api::CrossChainId;

const PAYLOAD_CACHE_EXPIRATION: u64 = 60 * 15; // 15 minutes
const PAYLOAD_CACHE_KEY_PREFIX: &str = "payload_cache";

pub struct PayloadCache {
    redis_pool: r2d2::Pool<redis::Client>,
}

impl PayloadCache {
    pub fn new(redis_pool: r2d2::Pool<redis::Client>) -> Self {
        Self { redis_pool }
    }

    fn get_key(&self, cc_id: CrossChainId) -> String {
        format!("{}:{}", PAYLOAD_CACHE_KEY_PREFIX, cc_id)
    }

    pub async fn get(&self, cc_id: CrossChainId) -> Result<Option<Vec<u8>>, anyhow::Error> {
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis connection: {}", e))?;

        let payload_bytes = redis_conn
            .get(self.get_key(cc_id))
            .map_err(|e| anyhow::anyhow!("Failed to get payload from Redis: {}", e))?;

        Ok(payload_bytes)
    }

    pub async fn store(
        &self,
        cc_id: CrossChainId,
        payload_bytes: Vec<u8>,
    ) -> Result<(), anyhow::Error> {
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis connection: {}", e))?;

        let _: () = redis_conn
            .set_ex(self.get_key(cc_id), payload_bytes, PAYLOAD_CACHE_EXPIRATION)
            .map_err(|e| anyhow::anyhow!("Failed to cache payload: {}", e))?;

        Ok(())
    }

    pub async fn clear(&self, cc_id: CrossChainId) -> Result<(), anyhow::Error> {
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis connection: {}", e))?;

        let _: () = redis_conn
            .del(self.get_key(cc_id))
            .map_err(|e| anyhow::anyhow!("Failed to clear payload cache: {}", e))?;

        Ok(())
    }
}
