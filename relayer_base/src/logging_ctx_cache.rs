/*!

Store tracing context cache in Redis, so it can be reused between services.

It accepts a list of message IDs and a context, and returns the context for the first message ID
that exists in Redis. If no message ID exists in Redis, it stores the context in Redis for
every message ID and returns the context.

get_or_insert will never fail - tracing is not important enough to stop the relayer or to
bundle up a lot of error handling code.

# Notes

It is tempting to create a cache.rs function that abstracts this away and accepts anything
serializable. However:

- We have a special handling because of multiple message_ids, and it's useful to encapsulate
  all logging ctx caching into a single trait.
- Redis is relatively easy to use as is.

If caching any serializable value without regard for failures becomes a common pattern,
we should move it to a cache.rs (or better yet, find a library that does it for us).
*/
use crate::logging::{distributed_tracing_headers_hash_map, hashmap_extract_parent_context};
use async_trait::async_trait;
use opentelemetry::Context;
use redis::{aio::ConnectionManager, AsyncCommands};
use std::collections::HashMap;
use tracing::{debug, error};

const EXPIRATION_TIME: u64 = 604800; // 7 days

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LoggingCtxCache: Send + Sync {
    async fn get_or_insert(
        &self,
        message_id: Vec<String>,
        ctx: HashMap<String, String>,
    ) -> Option<HashMap<String, String>>;

    async fn get_or_store_context(
        &self,
        message_id: Vec<String>,
        span: &tracing::Span,
    ) -> Option<Context> {
        let map = distributed_tracing_headers_hash_map(span);
        let map = self.get_or_insert(message_id, map).await?;
        Some(hashmap_extract_parent_context(&map))
    }
}

pub struct RedisLoggingCtxCache {
    redis_conn: ConnectionManager,
}

impl RedisLoggingCtxCache {
    pub fn new(redis_conn: ConnectionManager) -> Self {
        Self { redis_conn }
    }

    fn serialize_ctx(ctx: &HashMap<String, String>) -> Result<String, HashMap<String, String>> {
        let serialized = match serde_json::to_string(&ctx) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to serialize context: {}", e);
                return Err(ctx.clone());
            }
        };
        Ok(serialized)
    }

    async fn store_cache(
        redis_conn: &mut ConnectionManager,
        serialized: &String,
        message_id: String,
    ) {
        let key = format!("logging_ctx:{}", message_id);
        match redis_conn
            .set_ex::<_, _, ()>(&key, &serialized, EXPIRATION_TIME)
            .await
        {
            Ok(_) => {
                debug!("Stored context for message_id: {}", message_id);
            }
            Err(e) => {
                error!(
                    "Failed to store context in Redis for message_id {}: {}",
                    message_id, e
                );
            }
        }
    }

    fn deserialize_ctx(message_id: &str, serialized: &str) -> Option<HashMap<String, String>> {
        match serde_json::from_str::<HashMap<String, String>>(serialized) {
            Ok(deserialized_ctx) => {
                debug!("Found existing context for message_id: {}", message_id);
                return Some(deserialized_ctx);
            }
            Err(e) => {
                error!(
                    "Failed to deserialize context for message_id {}: {}",
                    message_id, e
                );
            }
        }
        None
    }
}

#[async_trait]
impl LoggingCtxCache for RedisLoggingCtxCache {
    async fn get_or_insert(
        &self,
        message_ids: Vec<String>,
        ctx: HashMap<String, String>,
    ) -> Option<HashMap<String, String>> {
        if message_ids.is_empty() {
            return None;
        }

        let mut redis_conn = self.redis_conn.clone();

        // Find an existing context for any of the message IDs
        for message_id in &message_ids {
            let key = format!("logging_ctx:{}", message_id);
            debug!("Looking for context for message_id: {}", message_id);
            match redis_conn.get::<_, String>(&key).await {
                Ok(serialized) => {
                    if let Some(value) = Self::deserialize_ctx(message_id, &serialized) {
                        return Some(value);
                    }
                }
                Err(e) => {
                    debug!(
                        "Failed to get context from Redis for message_id {}: {}",
                        message_id, e
                    );
                }
            }
        }

        let serialized = match Self::serialize_ctx(&ctx) {
            Ok(value) => value,
            Err(_) => return None,
        };

        for message_id in message_ids {
            Self::store_cache(&mut redis_conn, &serialized, message_id).await;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redis::Client;
    use std::collections::HashMap;
    use testcontainers::core::{IntoContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::GenericImage;

    #[tokio::test]
    async fn test_get_or_insert() {
        let container = GenericImage::new("redis", "7.2.4")
            .with_exposed_port(6379.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
            .start()
            .await
            .unwrap();

        let host = container.get_host().await.unwrap();
        let host_port = container.get_host_port_ipv4(6379).await.unwrap();

        let url = format!("redis://{host}:{host_port}");
        let client = Client::open(url.as_ref()).unwrap();

        let conn = client.get_connection_manager().await.unwrap();
        let mut test_conn = client.get_connection_manager().await.unwrap();

        let cache = RedisLoggingCtxCache::new(conn);

        // Create a test context
        let mut ctx = HashMap::new();
        ctx.insert("key1".to_string(), "value1".to_string());
        ctx.insert("key2".to_string(), "value2".to_string());

        let message_ids = vec!["1".to_string(), "2".to_string()];

        // Store context in redis (now it's empty)
        let result = cache.get_or_insert(message_ids.clone(), ctx.clone()).await;
        assert!(result.is_none(), "Should return None");

        // Verify that the context was stored in Redis
        let key = format!("logging_ctx:{}", message_ids[0]);
        let exists: bool = test_conn.exists(&key).await.unwrap();
        assert!(exists, "Context should be stored in Redis");

        // Create a different context
        let mut different_ctx = HashMap::new();
        different_ctx.insert("key3".to_string(), "value3".to_string());

        // Second call should retrieve the context from Redis
        let result = cache
            .get_or_insert(message_ids.clone(), different_ctx.clone())
            .await;
        assert_eq!(
            result.unwrap(),
            ctx,
            "Should return the original context from Redis"
        );

        // Test with a new message ID that doesn't exist in Redis
        let new_message_ids = vec!["3".to_string()];
        let result = cache
            .get_or_insert(new_message_ids.clone(), different_ctx.clone())
            .await;
        assert!(result.is_none(),);

        // Verify that the new context was stored in Redis
        let key = format!("logging_ctx:{}", new_message_ids[0]);
        let exists: bool = test_conn.exists(&key).await.unwrap();
        assert!(exists, "New context should be stored in Redis");
    }
}
