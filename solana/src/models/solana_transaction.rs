use std::future::Future;

use anyhow::Result;
use async_trait::async_trait;
use relayer_base::utils::ThreadSafe;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[async_trait]
pub trait SolanaTransactionModel: ThreadSafe {
    async fn find(&self, id: String) -> Result<Option<SolanaTransactionData>>;
    async fn upsert(&self, tx: SolanaTransactionData) -> Result<bool>;
    async fn delete(&self, tx: SolanaTransactionData) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SolanaTransactionData {
    pub signature: String,
    pub slot: i64,
    pub logs: Vec<String>,
    pub events: Vec<String>,
    pub retries: i32,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventSummary {
    pub event_id: String,
    pub message_id: Option<String>,
    pub event_type: String,
}

const PG_TABLE_NAME: &str = "solana_transactions";
#[derive(Debug, Clone)]
pub struct PgSolanaTransactionModel {
    pool: PgPool,
}

impl PgSolanaTransactionModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait UpdateEvents {
    fn update_events(
        &self,
        signature: String,
        events: Vec<EventSummary>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[async_trait]
impl SolanaTransactionModel for PgSolanaTransactionModel {
    async fn find(&self, id: String) -> Result<Option<SolanaTransactionData>> {
        let query = format!("SELECT * FROM {} WHERE signature = $1", PG_TABLE_NAME);
        let tx = sqlx::query_as::<_, SolanaTransactionData>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(tx)
    }

    async fn upsert(&self, tx: SolanaTransactionData) -> Result<bool> {
        let query = format!(
            "INSERT INTO {} \
             (signature, slot, logs, events, retries, created_at) \
             VALUES ($1, $2, $3, $4, $5, NOW()) \
             ON CONFLICT (signature) DO NOTHING \
            ",
            PG_TABLE_NAME
        );

        let res = sqlx::query(&query)
            .bind(tx.signature)
            .bind(tx.slot)
            .bind(tx.logs)
            .bind(tx.events)
            .bind(tx.retries)
            .execute(&self.pool)
            .await?;

        let inserted = res.rows_affected() > 0;

        Ok(inserted)
    }

    async fn delete(&self, tx: SolanaTransactionData) -> Result<()> {
        let query = format!("DELETE FROM {} WHERE signature = $1", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(tx.signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

impl UpdateEvents for PgSolanaTransactionModel {
    async fn update_events(&self, signature: String, events: Vec<EventSummary>) -> Result<()> {
        let query = format!(
            "UPDATE {} SET events = $1 WHERE signature = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(
                events
                    .into_iter()
                    .map(|e| e.event_id)
                    .collect::<Vec<String>>(),
            )
            .bind(signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
