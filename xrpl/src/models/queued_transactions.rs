use anyhow::Result;
//use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct QueuedTransaction {
    pub tx_hash: String,
    pub retries: i32,
    pub account: Option<String>,
    pub sequence: Option<i64>,
    pub status: Option<String>,
    pub last_checked: Option<DateTime<Utc>>,
}
//#[async_trait]
#[cfg_attr(any(test), mockall::automock)]
pub trait QueuedTransactionsModel {
    async fn get_queued_transactions_ready_for_check(&self) -> Result<Vec<QueuedTransaction>>;
    async fn mark_queued_transaction_confirmed(&self, tx_hash: &str) -> Result<()>;
    async fn mark_queued_transaction_dropped(&self, tx_hash: &str) -> Result<()>;
    async fn mark_queued_transaction_expired(&self, tx_hash: &str) -> Result<()>;
    async fn increment_queued_transaction_retry(&self, tx_hash: &str) -> Result<()>;
    async fn store_queued_transaction(
        &self,
        tx_hash: &str,
        account: &str,
        sequence: i64,
    ) -> Result<()>;
}

const PG_TABLE_NAME: &str = "xrpl_queued_transactions";

#[derive(Debug, Clone)]
pub struct PgQueuedTransactionsModel {
    pub pool: PgPool,
}

impl PgQueuedTransactionsModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl QueuedTransactionsModel for PgQueuedTransactionsModel {
    async fn get_queued_transactions_ready_for_check(&self) -> Result<Vec<QueuedTransaction>> {
        let query = format!("SELECT tx_hash, retries, account, sequence, status, last_checked FROM {}
                     WHERE status = 'queued'
                     AND (last_checked IS NULL OR last_checked < NOW() - INTERVAL '1 second' * POW(2, retries) * 10)", PG_TABLE_NAME);
        // 10 secs -> 20 secs -> 40 secs -> ...
        let transactions = sqlx::query_as::<_, QueuedTransaction>(&query)
            .fetch_all(&self.pool)
            .await?;
        Ok(transactions)
    }

    async fn mark_queued_transaction_confirmed(&self, tx_hash: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET status = 'confirmed', last_checked = now() WHERE tx_hash = $1",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_queued_transaction_dropped(&self, tx_hash: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET status = 'dropped', last_checked = now() WHERE tx_hash = $1",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_queued_transaction_expired(&self, tx_hash: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET status = 'expired', last_checked = now() WHERE tx_hash = $1",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn increment_queued_transaction_retry(&self, tx_hash: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET retries = retries + 1, last_checked = now() WHERE tx_hash = $1",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn store_queued_transaction(
        &self,
        tx_hash: &str,
        account: &str,
        sequence: i64,
    ) -> Result<()> {
        let query = format!("INSERT INTO {} (tx_hash, account, sequence, status) VALUES ($1, $2, $3, 'queued') ON CONFLICT (tx_hash) DO UPDATE SET account = $2, sequence = $3", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(tx_hash)
            .bind(account)
            .bind(sequence)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
