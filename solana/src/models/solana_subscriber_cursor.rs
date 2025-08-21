use anyhow::Result;
use sqlx::PgPool;
use std::future::Future;

#[cfg_attr(any(test), mockall::automock)]
pub trait SubscriberCursor {
    // Subscriber functions
    fn store_latest_signature(
        &self,
        chain: &str,
        context: &str,
        signature: String,
    ) -> impl Future<Output = Result<()>>;
    fn get_latest_signature(
        &self,
        chain: &str,
        context: &str,
    ) -> impl Future<Output = Result<Option<String>>>;
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

impl SubscriberCursor for PostgresDB {
    async fn store_latest_signature(
        &self,
        chain: &str,
        context: &str,
        signature: String,
    ) -> Result<()> {
        let query =
            "INSERT INTO subscriber_cursors (chain, context, signature) VALUES ($1, $2, $3) ON CONFLICT (chain, context) DO UPDATE SET signature = $3, updated_at = now() RETURNING chain, context, signature";

        sqlx::query(query)
            .bind(chain)
            .bind(context)
            .bind(signature)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_latest_signature(&self, chain: &str, context: &str) -> Result<Option<String>> {
        let query = "SELECT signature FROM subscriber_cursors WHERE chain = $1 AND context = $2";
        let signature = sqlx::query_scalar(query)
            .bind(chain)
            .bind(context)
            .fetch_optional(&self.pool)
            .await?;
        Ok(signature)
    }
}
