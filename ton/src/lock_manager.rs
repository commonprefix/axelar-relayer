/*!
Lock Manager with Redis implementation.

# Usage example

```rust,no_run
#[tokio::main]
async fn main() {
    use relayer_base::redis::connection_manager;
    use ton::lock_manager::{LockManager, RedisLockManager};

    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let conn = connection_manager(client, None, None, None).await.unwrap();
    let manager = RedisLockManager::new(conn);

    let lock = manager.lock("key").await;
    if lock {
        // We acquired the lock
    } else {
        // We failed to acquire the lock
    }
    manager.unlock("key").await;
}
```

# Notes

There are other Redis lock manager implementations available, but since this is a simple
functionality, maintaining our own will make it easier to customize the functionality in the future.

*/

use redis::AsyncCommands;
use async_trait::async_trait;
use redis::{ExistenceCheck, SetExpiry, SetOptions};
use redis::aio::ConnectionManager;
use tracing::error;

#[async_trait]
pub trait LockManager: Send + Sync {
    async fn lock(&self, key: &str) -> bool;
    async fn unlock(&self, key: &str);
}

pub struct RedisLockManager {
    conn: ConnectionManager,
}

impl RedisLockManager {
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    fn redis_connection(&self) -> ConnectionManager {
        self.conn.clone()
    }
}

#[async_trait::async_trait]
impl LockManager for RedisLockManager {
    async fn lock(&self, key: &str) -> bool {

        let set_opts = SetOptions::default()
            .conditional_set(ExistenceCheck::NX)
            .with_expiration(SetExpiry::EX(60));

        self.redis_connection()
            .set_options(format!("wallet_lock_{}", key), true, set_opts).await
            .unwrap_or_else(|e| {
                error!("Failed to set Redis lock: {}", e);
                false
            })
    }

    async fn unlock(&self, key: &str) {
        self.redis_connection()
            .del(format!("wallet_lock_{}", key)).await
            .unwrap_or_else(|e| {
                error!("Failed to set Redis lock: {}", e);
                false
            });
    }
}

#[cfg(test)]
mod tests {
    use crate::lock_manager::{LockManager, RedisLockManager};
    use redis::Client;
    use std::time::Duration;
    use testcontainers::{
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
        GenericImage,
    };
    use relayer_base::redis::connection_manager;

    async fn create_redis_lock_manager() -> (
        testcontainers::ContainerAsync<GenericImage>,
        RedisLockManager,
    ) {
        let container = GenericImage::new("redis", "7.2.4")
            .with_exposed_port(9991.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
            .start()
            .await
            .unwrap();

        let host = container.get_host().await.unwrap();
        let host_port = container.get_host_port_ipv4(6379).await.unwrap();

        let url = format!("redis://{host}:{host_port}");
        let client = Client::open(url.as_ref()).unwrap();

        let conn = connection_manager(client, Some(Duration::from_millis(100)), Some(Duration::from_millis(100)), Some(0)).await.unwrap();
        let manager = RedisLockManager::new(conn);

        (container, manager)
    }

    /// Both positive and negative tests are crammed in here so we save time on container creation
    #[tokio::test]
    async fn test_lock() {
        let (container, manager) = create_redis_lock_manager().await;

        let first = manager.lock("wallet1").await;
        assert!(first, "Should be able to acquire lock");

        let different = manager.lock("wallet2").await;
        assert!(different, "Should be able to acquire unrelated lock");

        let second = manager.lock("wallet1").await;
        assert!(!second, "Should fail because already locked");

        manager.unlock("wallet1").await;

        let different_f = manager.lock("wallet2").await;
        assert!(!different_f, "We should only release one lock");

        let third = manager.lock("wallet1").await;
        assert!(third, "Should be able to reacquire lock");

        container.stop_with_timeout(Some(1)).await.unwrap();

        let locked = manager.lock("test_key").await;
        assert!(!locked, "Lock should fail when Redis is not reachable");

        // We shouldn't fail when unlocking
        manager.unlock("test_key").await;
    }
}
