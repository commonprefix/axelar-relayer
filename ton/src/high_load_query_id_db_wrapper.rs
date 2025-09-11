/*!

DB Wrapper for high load query id. Stores a query id per address in the database and allows user
to increase it. Each wallet has a timeout. If the timeout has passed, the query id is reset to 0, 0.

This code *must be used* in conjuction with the `WalletManager`.

The TIMEOUT_BUFFER_MULTIPLIER is set to 3 for the following reason:

`
    if (last_clean_time < (now() - timeout)) {
        (old_queries, queries) = (queries, null());
        if (last_clean_time < (now() - (timeout * 2))) {
            old_queries = null();
        }
        last_clean_time = now();
    }
`

That means that it is only safe to reset query_id after 3 * timeout:

| Time           | Action                       | `queries` Contains | `old_queries` Contains | Can Send `X`? | Can Send `Y`? |
|----------------|------------------------------|---------------------|-------------------------|----------------|----------------|
| `t = 0`        | Send `X`                     | `X`                 | —                       | ❌ (was just used) | ✅ (not yet used) |
| `t = T`        | Send `Y`                     | `Y`                 | `X`                     | ❌ (in old_queries) | ❌ (was just used) |
| `t = 2T`       | `X` evicted from old_queries | —                   | `Y`                     | ✅ (fully expired) | ❌ (in old_queries) |
| `t = 3T`       | `Y` evicted from old_queries | —                   | —                       | ✅              | ✅              |

# Usage Example

```rust,no_run
use ton::config::WalletConfig;
use std::sync::Arc;
use ton::lock_manager::RedisLockManager;
use ton::wallet_manager::WalletManager;
use relayer_base::database::PostgresDB;
use relayer_base::redis::connection_manager;
use ton::high_load_query_id_db_wrapper::HighLoadQueryIdDbWrapper;
use ton::high_load_query_id_db_wrapper::HighLoadQueryIdWrapper;
use sqlx::PgPool;
use ton::ton_wallet_query_id::PgTONWalletQueryIdModel;

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
    let conn = connection_manager(client, None, None, None).await.unwrap();

    let lock_manager = Arc::new(RedisLockManager::new(conn));
    let wallet_manager = WalletManager::new(config, lock_manager).await;

    let pg_pool = PgPool::connect("psql://foo?bar").await.unwrap();
    let model = PgTONWalletQueryIdModel::new(pg_pool);
    let wrapper = HighLoadQueryIdDbWrapper::new(model).await;

    match wallet_manager.acquire().await {
        Ok(wallet) => {
            let timeout = 60 * 60;
            let query_id = wrapper.next("wallet1", timeout).await.unwrap();

            wallet_manager.release(wallet).await;
        }
        Err(e) => println!("Error acquiring wallet: {:?}", e),
    }
}
```

# See also
- https://docs.ton.org/v3/guidelines/smart-contracts/howto/wallet#replay-protection

*/

const TIMEOUT_BUFFER_MULTIPLIER: i32 = 3;

use crate::high_load_query_id::HighLoadQueryId;
use crate::models::ton_wallet_query_id::{PgTONWalletQueryIdModel, TONWalletQueryId};
use async_trait::async_trait;

#[derive(Debug)]
pub enum HighLoadQueryIdWrapperError {
    ConstructionError,
    NoNextQueryId,
    DatabaseError,
}

pub struct HighLoadQueryIdDbWrapper {
    model: PgTONWalletQueryIdModel,
}

#[cfg_attr(any(test), mockall::automock)]
#[async_trait]
pub trait HighLoadQueryIdWrapper: Send + Sync {
    async fn next(
        &self,
        address: &str,
        timeout: u64,
    ) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError>;
}

impl HighLoadQueryIdDbWrapper {
    pub async fn new(model: PgTONWalletQueryIdModel) -> Self {
        Self { model }
    }
}

#[async_trait]
impl HighLoadQueryIdWrapper for HighLoadQueryIdDbWrapper {
    async fn next(
        &self,
        address: &str,
        timeout: u64,
    ) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError> {
        let (shift, bitnumber) = self
            .model
            .get_query_id(address)
            .await
            .map_err(|_| HighLoadQueryIdWrapperError::DatabaseError)?;

        let query_id = if shift < 0 || bitnumber < 0 {
            HighLoadQueryId::from_shift_and_bitnumber(0u32, 0u32)
                .await
                .map_err(|_e| HighLoadQueryIdWrapperError::ConstructionError)?
        } else {
            let query_id =
                HighLoadQueryId::from_shift_and_bitnumber(shift as u32, bitnumber as u32)
                    .await
                    .map_err(|_e| HighLoadQueryIdWrapperError::ConstructionError)?;

            if query_id.has_next().await {
                query_id
                    .next()
                    .await
                    .map_err(|_| HighLoadQueryIdWrapperError::ConstructionError)?
            } else {
                return Err(HighLoadQueryIdWrapperError::NoNextQueryId);
            }
        };

        if query_id.query_id().await == 0 {
            self.model
                .upsert_query_id(
                    address,
                    query_id.shift as i32,
                    query_id.bitnumber as i32,
                    timeout as i32 * TIMEOUT_BUFFER_MULTIPLIER,
                )
                .await
                .map_err(|_e| HighLoadQueryIdWrapperError::DatabaseError)?;
        } else {
            self.model
                .update_query_id(address, query_id.shift as i32, query_id.bitnumber as i32)
                .await
                .map_err(|_e| HighLoadQueryIdWrapperError::DatabaseError)?;
        }

        Ok(query_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::high_load_query_id_db_wrapper::{HighLoadQueryIdDbWrapper, HighLoadQueryIdWrapper};
    use crate::ton_wallet_query_id::PgTONWalletQueryIdModel;
    use sqlx::PgPool;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres;

    #[tokio::test]
    async fn test_next() {
        let container = postgres::Postgres::default()
            .with_init_sql(
                include_str!("../../migrations/0010_ton_wallet_query_id.sql")
                    .to_string()
                    .into_bytes(),
            )
            .start()
            .await
            .unwrap();
        let connection_string = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await.unwrap(),
            container.get_host_port_ipv4(5432).await.unwrap()
        );

        let pg_pool = PgPool::connect(&connection_string).await.unwrap();
        let model = PgTONWalletQueryIdModel::new(pg_pool);

        let wrapper = HighLoadQueryIdDbWrapper::new(model).await;

        let query_id_a = wrapper.next("wallet1", 60).await.unwrap();
        assert_eq!(query_id_a.query_id().await, 0);
        let query_id_b = wrapper.next("wallet2", 60).await.unwrap();
        assert_eq!(query_id_b.query_id().await, 0);
        let query_id_c = wrapper.next("wallet1", 60).await.unwrap();
        assert_eq!(query_id_c.query_id().await, 1);

        let query_id_d = wrapper.next("wallet3", 0).await.unwrap();
        assert_eq!(query_id_d.query_id().await, 0);

        let query_id_e = wrapper.next("wallet3", 60).await.unwrap();
        assert_eq!(query_id_e.query_id().await, 0);
    }
}
