use anyhow::Result;
use async_trait::async_trait;
use router_api::CrossChainId;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::str::FromStr;

// TODO: split to models
#[cfg_attr(any(test, feature = "mocks"), mockall::automock)]
#[async_trait]
pub trait Database {
    // Subscriber functions
    async fn store_latest_height(&self, chain: &str, context: &str, height: i64) -> Result<()>;
    async fn get_latest_height(&self, chain: &str, context: &str) -> Result<Option<i64>>;

    // Distributor functions
    async fn store_latest_task_id(&self, chain: &str, context: &str, task_id: &str) -> Result<()>;
    async fn get_latest_task_id(&self, chain: &str, context: &str) -> Result<Option<String>>;

    // Payload cache functions
    async fn store_payload(&self, cc_id: CrossChainId, value: String) -> Result<()>;
    async fn get_payload(&self, cc_id: CrossChainId) -> Result<Option<String>>;
    async fn clear_payload(&self, cc_id: CrossChainId) -> Result<()>;

    // Price view functions
    async fn get_price(&self, pair: &str) -> Result<Option<Decimal>>;
    async fn store_price(&self, pair: &str, price: Decimal) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct PostgresDB {
    pool: PgPool,
}

impl PostgresDB {
    pub async fn new(url: &str) -> Result<Self> {
        let pool = PgPool::connect(url).await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl Database for PostgresDB {
    async fn store_latest_height(&self, chain: &str, context: &str, height: i64) -> Result<()> {
        let query =
            "INSERT INTO subscriber_cursors (chain, context, height) VALUES ($1, $2, $3) ON CONFLICT (chain, context) DO UPDATE SET height = $3, updated_at = now() RETURNING chain, context, height";

        sqlx::query(query)
            .bind(chain)
            .bind(context)
            .bind(height)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_latest_height(&self, chain: &str, context: &str) -> Result<Option<i64>> {
        let query = "SELECT height FROM subscriber_cursors WHERE chain = $1 AND context = $2";
        let height = sqlx::query_scalar(query)
            .bind(chain)
            .bind(context)
            .fetch_optional(&self.pool)
            .await?;
        Ok(height)
    }

    async fn store_latest_task_id(&self, chain: &str, context: &str, task_id: &str) -> Result<()> {
        let query = "INSERT INTO distributor_cursors (chain, context, task_id) VALUES ($1, $2, $3) ON CONFLICT (chain, context) DO UPDATE SET task_id = $3, updated_at = now()";
        sqlx::query(query)
            .bind(chain)
            .bind(context)
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_latest_task_id(&self, chain: &str, context: &str) -> Result<Option<String>> {
        let query = "SELECT task_id FROM distributor_cursors WHERE chain = $1 AND context = $2";
        let task_id = sqlx::query_scalar(query)
            .bind(chain)
            .bind(context)
            .fetch_optional(&self.pool)
            .await?;
        Ok(task_id)
    }

    async fn get_payload(&self, cc_id: CrossChainId) -> Result<Option<String>> {
        let query = "SELECT message_with_payload FROM messages_with_payload WHERE cc_id = $1";
        let payload = sqlx::query_scalar(query)
            .bind(cc_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(payload)
    }

    async fn store_payload(&self, cc_id: CrossChainId, value: String) -> Result<()> {
        let query = "INSERT INTO messages_with_payload (cc_id, message_with_payload) VALUES ($1, $2) ON CONFLICT (cc_id) DO UPDATE SET message_with_payload = $2, updated_at = now()";
        sqlx::query(query)
            .bind(cc_id.to_string())
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear_payload(&self, cc_id: CrossChainId) -> Result<()> {
        let query = "DELETE FROM messages_with_payload WHERE cc_id = $1";
        sqlx::query(query)
            .bind(cc_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn store_price(&self, pair: &str, price: Decimal) -> Result<()> {
        let query = "INSERT INTO pair_prices (pair, price) VALUES ($1, $2) ON CONFLICT (pair) DO UPDATE SET price = $2, updated_at = now()";
        sqlx::query(query)
            .bind(pair)
            .bind(price.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_price(&self, pair: &str) -> Result<Option<Decimal>> {
        let query = "SELECT price FROM pair_prices WHERE pair = $1";
        let maybe_row = sqlx::query(query)
            .bind(pair)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = maybe_row {
            let price: String = row.try_get("price")?;
            Ok(Some(Decimal::from_str(&price)?))
        } else {
            Ok(None)
        }
    }
}
