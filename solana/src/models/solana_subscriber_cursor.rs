use anyhow::Result;
use sqlx::PgPool;
use std::future::Future;

#[cfg_attr(any(test), mockall::automock)]
pub trait SubscriberCursor {
    // Subscriber functions
    fn store_latest_signature(
        &self,
        context: &str,
        signature: String,
    ) -> impl Future<Output = Result<()>>;
    fn get_latest_signature(&self, context: &str) -> impl Future<Output = Result<Option<String>>>;
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
    async fn store_latest_signature(&self, context: &str, signature: String) -> Result<()> {
        let query =
            "INSERT INTO solana_subscriber_cursors (context, signature) VALUES ($1, $2) ON CONFLICT (context) DO UPDATE SET signature = $2, updated_at = now() RETURNING context, signature";

        sqlx::query(query)
            .bind(context)
            .bind(signature)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_latest_signature(&self, context: &str) -> Result<Option<String>> {
        let query = "SELECT signature FROM solana_subscriber_cursors WHERE context = $1";
        let signature = sqlx::query_scalar(query)
            .bind(context)
            .fetch_optional(&self.pool)
            .await?;
        Ok(signature)
    }
}
