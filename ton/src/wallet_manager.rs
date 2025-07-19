/*!
This module provides the `WalletManager` struct for managing a pool of `TonWalletHighLoadV3` wallets.
It ensures safe and exclusive access to wallets using a pluggable locking mechanism (`LockManager`).

# Components
- `WalletConfig`: Configuration for a single wallet.
- `WalletManager`: Loads and manages multiple wallets, providing acquire/release logic.
- `WalletManagerError`: Error type for wallet management operations.
- `LockManager`: Trait for implementing custom lock strategies (e.g., memory, Redis, etc).
- `TonWalletHighLoadV3`: Represents a high-throughput TON wallet used for sending transactions.

# Usage Example

```rust,no_run
use ton::config::WalletConfig;
use std::sync::Arc;
use ton::lock_manager::RedisLockManager;
use ton::wallet_manager::WalletManager;
use tracing::error;

#[tokio::main]
async fn main() {
    let config = vec![
        WalletConfig {
            public_key: "abcd1234".into(),
            secret_key: "1234abcd".into(),
            address: "EQ...".into(),
            subwallet_id: 1,
            timeout: 30,
        },
    ];

    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let pool = r2d2::Pool::builder().build(client).unwrap();

    let lock_manager = Arc::new(RedisLockManager::new(pool));
    let wallet_manager = WalletManager::new(config, lock_manager).await;

    match wallet_manager.acquire().await {
        Ok(wallet) => {
            wallet_manager.release(wallet).await;
        }
        Err(e) => error!("Error acquiring wallet: {:?}", e),
    }
}
```

# TODO

- Add `acquire_skip(wallets_to_skip: &[&TonWalletHighLoadV3])` method, so that unusable wallets can be taken out of rotation.
- Potentially add round robin selection, although it is questionable whether it would help us in practice.

# Notes

This would have been a much better implementation using RAII instead of the `release` method.
However, RAII requires a `Drop` trait, which cannot be asynchronous, so it would be hard to
ensure timely unlocking in case when LockManager is network-bound, like RedisLockManager.

There is an example of async drop in testcontainers: https://github.com/testcontainers/testcontainers-rs/blob/main/testcontainers/src/core/async_drop.rs#L16
However, it seems a little bit unfair to spawn a thread every time we want ot release a lock.

# Potential for reuse

`WalletManager` could become `ResourceManager` by templating away the wallet type and providing
an explicit way to specify key for each resource type.

*/

use crate::config::WalletConfig;
use crate::lock_manager::LockManager;
use crate::ton_wallet_high_load_v3::TonWalletHighLoadV3;
use hex::decode;
use std::collections::HashMap;
use std::sync::Arc;
use tonlib_core::wallet::mnemonic::KeyPair;
use tonlib_core::TonAddress;
use tracing::debug;

#[derive(Debug)]
pub enum WalletManagerError {
    NoAvailableWallet,
    LockError(String),
}

pub struct WalletManager {
    wallets: HashMap<TonAddress, TonWalletHighLoadV3>,
    lock_manager: Arc<dyn LockManager>,
}

impl WalletManager {
    pub async fn new(config: Vec<WalletConfig>, lock_manager: Arc<dyn LockManager>) -> Self {
        let mut wallets = HashMap::new();

        for c in config {
            let wallet = Self::load_wallet(c.clone());
            wallets.insert(wallet.address.clone(), wallet);
        }

        Self {
            wallets,
            lock_manager,
        }
    }

    fn load_wallet(config: WalletConfig) -> TonWalletHighLoadV3 {
        let public_key_bytes = decode(&config.public_key).unwrap();
        let secret_key_bytes = decode(&config.secret_key).unwrap();
        let key_pair = KeyPair {
            public_key: public_key_bytes,
            secret_key: secret_key_bytes,
        };

        TonWalletHighLoadV3::new(
            TonAddress::from_base64_url(&config.address).unwrap(),
            key_pair,
            config.subwallet_id,
            config.timeout,
        )
    }

    pub async fn acquire(&self) -> Result<&TonWalletHighLoadV3, WalletManagerError> {
        for (address, wallet) in &self.wallets {
            let l = self.lock_manager.lock(&address.to_string()).await;
            if l {
                debug!("Acquired wallet: {:?}", address);
                return Ok(wallet);
            }
        }
        Err(WalletManagerError::NoAvailableWallet)
    }

    pub async fn release(&self, wallet: &TonWalletHighLoadV3) {
        self.lock_manager.unlock(&wallet.address.to_string()).await;
        debug!("Released wallet: {:?}", wallet.address);
    }
}

#[cfg(test)]
pub(crate) mod wallet_manager_tests {
    use crate::config::WalletConfig;
    use crate::lock_manager::LockManager;
    use crate::wallet_manager::{WalletManager, WalletManagerError};
    use std::any::type_name;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    pub struct MockLockManager {
        locked: Mutex<HashSet<String>>,
    }

    #[async_trait::async_trait]
    impl LockManager for MockLockManager {
        async fn lock(&self, key: &str) -> bool {
            let mut locked = self.locked.lock().unwrap();
            if locked.contains(key) {
                false
            } else {
                locked.insert(key.to_string());
                true
            }
        }

        async fn unlock(&self, key: &str) {
            let mut locked = self.locked.lock().unwrap();
            locked.remove(key);
        }
    }

    fn type_of<T>(_: &T) -> &'static str {
        type_name::<T>()
    }

    pub(crate) async fn load_wallets() -> WalletManager {
        let wallet_data = vec![
            (1, 30, "EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c"),
            (2, 60, "EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADz6z"),
            (3, 90, "EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA_9Gs"),
        ];

        let wallets: Vec<WalletConfig> = wallet_data
            .into_iter()
            .map(|(id, timeout, address)| WalletConfig {
                public_key: "0fff".to_string(),
                secret_key: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
                subwallet_id: id,
                timeout,
                address: address.to_string(),
            })
            .collect();

        let lock_manager = Arc::new(MockLockManager {
            locked: Mutex::new(HashSet::new()),
        });
        WalletManager::new(wallets, lock_manager).await
    }

    #[tokio::test]
    async fn test_load_wallets() {
        let wallet_manager = load_wallets().await;
        assert_eq!(wallet_manager.wallets.len(), 3);

        for wallet in wallet_manager.wallets.values() {
            assert_eq!(
                type_of(&wallet.address),
                "tonlib_core::types::address::TonAddress"
            );
            assert_eq!(
                type_of(&wallet.key_pair),
                "tonlib_core::wallet::mnemonic::KeyPair"
            );
            assert_eq!(type_of(&wallet.subwallet_id), "u32");
            assert_eq!(type_of(&wallet.timeout), "u64");
            assert_eq!(
                type_of(&wallet.time_provider),
                "ton::ton_wallet_high_load_v3::SystemTimeProvider"
            );
        }
    }

    #[tokio::test]
    async fn test_acquire_wallet() {
        let wallet_manager = load_wallets().await;
        let w1 = wallet_manager
            .acquire()
            .await
            .expect("Should acquire wallet 1");
        wallet_manager.release(&w1).await;
        let w2 = wallet_manager.acquire().await.expect("wallet 2 ok");
        let w3 = wallet_manager.acquire().await.expect("wallet 3 ok");
        let _w4 = wallet_manager.acquire().await.expect("wallet 4 ok");
        wallet_manager.release(&w2).await;
        let _w5 = wallet_manager.acquire().await.expect("wallet 5 ok");
        let result = wallet_manager.acquire().await;
        assert!(matches!(result, Err(WalletManagerError::NoAvailableWallet)));
        wallet_manager.release(&w3).await;
        let _w6 = wallet_manager.acquire().await.expect("wallet 6 ok");
    }
}
