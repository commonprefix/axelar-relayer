/*!

DB Wrapper for high load query id. Stores a query id per address in the database and allows user
to increase it. Each wallet has a timeout. If the timeout has passed, the query id is reset to 0, 0.

This code *must be used* in conjuction with the `WalletManager`.

# Usage Example

```rust
let wallet_manager = WalletManager::new(config, lock_manager).await;
let postgres_db = PostgresDB::new(&connection_string).await.unwrap();
let wrapper = HighLoadQueryIdDbWrapper::new(postgres_db.clone(), 60).await;

match wallet_manager.acquire().await {
    Ok(wallet) => {
        let query_id = wrapper.next("wallet1").await.unwrap();
        // use query_id to send a message to the blockchain
        wallet_manager.release(wallet).await;
    }
    Err(e) => error!("Error acquiring wallet: {:?}", e),
}
```

# TODO
- [ ] Test how timeout (the time after which queries are moved to old_queries) works in practice.

# See also
- https://docs.ton.org/v3/guidelines/smart-contracts/howto/wallet#replay-protection

*/

use async_trait::async_trait;
use crate::high_load_query_id::HighLoadQueryId;
use relayer_base::database::{Database, PostgresDB};

#[derive(Debug)]
pub enum HighLoadQueryIdWrapperError {
    ConstructionError,
    NoNextQueryId,
}

pub struct HighLoadQueryIdDbWrapper {
    db: PostgresDB,
}

#[async_trait]
pub trait HighLoadQueryIdWrapper {
    async fn next(&self, address: &str, timeout: u64) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError>;
}

impl HighLoadQueryIdDbWrapper {
    pub async fn new(db: PostgresDB) -> Self {
        Self { db }
    }
}

#[async_trait]
impl HighLoadQueryIdWrapper for HighLoadQueryIdDbWrapper {
    async fn next(&self, address: &str, timeout: u64) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError> {
        let (shift, bitnumber) = self.db.get_query_id(address).await.unwrap();

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
            self.db
                .upsert_query_id(
                    address,
                    query_id.shift as i32,
                    query_id.bitnumber as i32,
                    timeout as i32,
                )
                .await
                .unwrap();
        } else {
            self.db
                .update_query_id(address, query_id.shift as i32, query_id.bitnumber as i32)
                .await
                .unwrap();
        }

        Ok(query_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::high_load_query_id_db_wrapper::{HighLoadQueryIdDbWrapper, HighLoadQueryIdWrapper};
    use relayer_base::database::PostgresDB;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres;

    #[tokio::test]
    async fn test_next() {
        let container = postgres::Postgres::default()
            .with_init_sql(
                include_str!("../../migrations/0009_ton_wallet_query_id.sql")
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

        let postgres_db = PostgresDB::new(&connection_string).await.unwrap();

        let wrapper = HighLoadQueryIdDbWrapper::new(postgres_db.clone()).await;

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
