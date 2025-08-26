use anyhow::Result;
use sqlx::PgPool;
use std::future::Future;

#[derive(Clone, Debug, sqlx::Type)]
#[sqlx(type_name = "subscriber_mode")]
pub enum SubscriberMode {
    Stream,
    Poll,
}

#[cfg_attr(any(test), mockall::automock)]
pub trait SubscriberCursor {
    // Subscriber functions
    fn store_latest_signature(
        &self,
        context: String,
        signature: String,
        mode: SubscriberMode,
    ) -> impl Future<Output = Result<()>>;
    fn get_latest_signature(
        &self,
        context: String,
        mode: SubscriberMode,
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
        context: String,
        signature: String,
        mode: SubscriberMode,
    ) -> Result<()> {
        let query =
            "INSERT INTO solana_subscriber_cursors (context, signature, mode) VALUES ($1, $2, $3) ON CONFLICT (context, mode) DO UPDATE SET signature = $2, updated_at = now() RETURNING context, signature";

        sqlx::query(query)
            .bind(context)
            .bind(signature)
            .bind(mode)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_latest_signature(
        &self,
        context: String,
        mode: SubscriberMode,
    ) -> Result<Option<String>> {
        let query =
            "SELECT signature FROM solana_subscriber_cursors WHERE context = $1 AND mode = $2";
        let signature = sqlx::query_scalar(query)
            .bind(context)
            .bind(mode)
            .fetch_optional(&self.pool)
            .await?;
        Ok(signature)
    }
}
