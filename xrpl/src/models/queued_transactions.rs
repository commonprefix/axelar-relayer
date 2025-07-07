use std::future::Future;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct QueuedTransaction {
    pub tx_hash: String,
    pub retries: i32,
    pub account: Option<String>,
    pub sequence: Option<i64>,
    pub status: QueuedTransactionStatus,
    pub last_checked: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::Type, PartialEq)]
#[sqlx(type_name = "xrpl_queued_transaction_status")]
pub enum QueuedTransactionStatus {
    Queued,
    Confirmed,
    Dropped,
    Expired,
}

#[cfg_attr(any(test), mockall::automock)]
pub trait QueuedTransactionsModel {
    fn update_transaction_status(
        &self,
        tx_hash: &str,
        status: QueuedTransactionStatus,
    ) -> impl Future<Output = Result<()>>;
    fn get_queued_transactions_ready_for_check(
        &self,
    ) -> impl Future<Output = Result<Vec<QueuedTransaction>>>;
    fn store_queued_transaction(
        &self,
        tx_hash: &str,
        account: &str,
        sequence: i64,
    ) -> impl Future<Output = Result<()>>;
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

    async fn update_transaction_status(
        &self,
        tx_hash: &str,
        status: QueuedTransactionStatus,
    ) -> Result<()> {
        let query = format!(
            "UPDATE {} SET status = $1, last_checked = now() WHERE tx_hash = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(status)
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
