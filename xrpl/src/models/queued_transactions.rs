use std::future::Future;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, PartialEq)]
pub struct QueuedTransaction {
    pub tx_hash: String,
    pub retries: i32,
    pub account: Option<String>,
    pub sequence: Option<i64>,
    pub status: QueuedTransactionStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::Type, PartialEq)]
#[sqlx(type_name = "xrpl_queued_transaction_status")]
pub enum QueuedTransactionStatus {
    #[sqlx(rename = "queued")]
    Queued,
    #[sqlx(rename = "confirmed")]
    Confirmed,
    #[sqlx(rename = "dropped")]
    Dropped,
    #[sqlx(rename = "expired")]
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
    fn increment_retry(&self, tx_hash: &str) -> impl Future<Output = Result<()>>;
}

const PG_TABLE_NAME: &str = "xrpl_queued_transactions";

#[derive(Debug, Clone)]
pub struct PgQueuedTransactionsModel {
    pool: PgPool,
}

impl PgQueuedTransactionsModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl QueuedTransactionsModel for PgQueuedTransactionsModel {
    async fn get_queued_transactions_ready_for_check(&self) -> Result<Vec<QueuedTransaction>> {
        let query = format!("SELECT tx_hash, retries, account, sequence, status FROM {}
                     WHERE status = $1
                     AND (last_checked IS NULL OR last_checked < NOW() - INTERVAL '1 second' * POW(2, retries) * 10)", PG_TABLE_NAME);
        // 10 secs -> 20 secs -> 40 secs -> ...
        let transactions = sqlx::query_as::<_, QueuedTransaction>(&query)
            .bind(QueuedTransactionStatus::Queued)
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
        let query = format!("INSERT INTO {} (tx_hash, account, sequence, status) VALUES ($1, $2, $3, $4) ON CONFLICT (tx_hash) DO UPDATE SET account = $2, sequence = $3", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(tx_hash)
            .bind(account)
            .bind(sequence)
            .bind(QueuedTransactionStatus::Queued)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn increment_retry(&self, tx_hash: &str) -> Result<()> {
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
}

#[cfg(test)]
mod tests {

    use sqlx::PgPool;
    use testcontainers::{runners::AsyncRunner, ContainerAsync};
    use testcontainers_modules::postgres;

    use crate::models::queued_transactions::{
        PgQueuedTransactionsModel, QueuedTransaction, QueuedTransactionStatus,
        QueuedTransactionsModel,
    };

    async fn setup_test_container() -> (
        PgQueuedTransactionsModel,
        ContainerAsync<postgres::Postgres>,
    ) {
        let container = postgres::Postgres::default()
            .with_init_sql(
                include_str!("../../../migrations/0009_xrpl_queued_transactions.sql")
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
        let pool = sqlx::PgPool::connect(&connection_string).await.unwrap();
        let model = PgQueuedTransactionsModel::new(pool);
        // we need to return the container too otherwise it will be dropped and the test will run forever
        (model, container)
    }

    async fn simulate_time_passed_with_update_last_checked(
        tx_hash: &str,
        pool: &PgPool,
        seconds: u64,
    ) {
        sqlx::query(&format!(
            "UPDATE xrpl_queued_transactions SET last_checked = now() - INTERVAL '{} seconds' WHERE tx_hash = $1"
        , seconds))
        .bind(tx_hash)
        .execute(pool)
        .await.unwrap();
    }

    #[tokio::test]
    async fn test_store_queued_transaction() {
        let (queued_tx_model, _container) = setup_test_container().await;
        let tx1 = QueuedTransaction {
            tx_hash: "test_hash".to_string(),
            retries: 0,
            account: Some("test_account".to_string()),
            sequence: Some(123),
            status: QueuedTransactionStatus::Queued,
        };

        queued_tx_model
            .store_queued_transaction(
                tx1.tx_hash.as_str(),
                tx1.account.as_ref().unwrap(),
                tx1.sequence.unwrap(),
            )
            .await
            .unwrap();

        let txs: Vec<QueuedTransaction> =
            sqlx::query_as::<_, QueuedTransaction>("SELECT * FROM xrpl_queued_transactions")
                .fetch_all(&queued_tx_model.pool)
                .await
                .unwrap();

        assert_eq!(txs.len(), 1);
        assert_eq!(tx1, txs[0]);
    }

    #[tokio::test]
    async fn test_store_queued_transaction_and_get_ready_for_check() {
        let (queued_tx_model, _container) = setup_test_container().await;
        let tx1 = QueuedTransaction {
            tx_hash: "test_hash".to_string(),
            retries: 0,
            account: Some("test_account".to_string()),
            sequence: Some(123),
            status: QueuedTransactionStatus::Queued,
        };

        let tx2 = QueuedTransaction {
            tx_hash: "test_hash2".to_string(),
            retries: 0,
            account: Some("test_account2".to_string()),
            sequence: Some(124),
            status: QueuedTransactionStatus::Queued,
        };

        queued_tx_model
            .store_queued_transaction(
                tx1.tx_hash.as_str(),
                tx1.account.as_ref().unwrap(),
                tx1.sequence.unwrap(),
            )
            .await
            .unwrap();

        queued_tx_model
            .store_queued_transaction(
                tx2.tx_hash.as_str(),
                tx2.account.as_ref().unwrap(),
                tx2.sequence.unwrap(),
            )
            .await
            .unwrap();

        let transactions = queued_tx_model
            .get_queued_transactions_ready_for_check()
            .await
            .unwrap();

        // no tx should be ready for check since last_checked is now()
        assert_eq!(transactions.len(), 0);

        // tx1 starts with retries = 0 and tx2 starts with retries = 1
        queued_tx_model
            .increment_retry(tx2.tx_hash.as_str())
            .await
            .unwrap();

        // simulate 10 seconds passing since the last check
        simulate_time_passed_with_update_last_checked(&tx1.tx_hash, &queued_tx_model.pool, 10)
            .await;
        simulate_time_passed_with_update_last_checked(&tx2.tx_hash, &queued_tx_model.pool, 10)
            .await;

        let transactions = queued_tx_model
            .get_queued_transactions_ready_for_check()
            .await
            .unwrap();

        // only 1 transaction should be ready for check since tx2 was checked now()
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0], tx1);

        // simulate 20 seconds passing since the last check
        simulate_time_passed_with_update_last_checked(&tx1.tx_hash, &queued_tx_model.pool, 20)
            .await;
        simulate_time_passed_with_update_last_checked(&tx2.tx_hash, &queued_tx_model.pool, 20)
            .await;

        let transactions = queued_tx_model
            .get_queued_transactions_ready_for_check()
            .await
            .unwrap();

        // both tx should be ready after 20 seconds
        assert_eq!(transactions.len(), 2);
        assert_eq!(transactions[0].tx_hash, tx1.tx_hash);
        assert_eq!(transactions[1].tx_hash, tx2.tx_hash);

        queued_tx_model
            .increment_retry(tx2.tx_hash.as_str())
            .await
            .unwrap();

        // simulate 30 seconds passing since the last check
        simulate_time_passed_with_update_last_checked(&tx1.tx_hash, &queued_tx_model.pool, 30)
            .await;
        simulate_time_passed_with_update_last_checked(&tx2.tx_hash, &queued_tx_model.pool, 30)
            .await;

        let transactions = queued_tx_model
            .get_queued_transactions_ready_for_check()
            .await
            .unwrap();

        // only 1 transaction should be ready after 30 seconds
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].tx_hash, tx1.tx_hash);

        // simulate 40 seconds passing since the last check
        simulate_time_passed_with_update_last_checked(&tx1.tx_hash, &queued_tx_model.pool, 40)
            .await;
        simulate_time_passed_with_update_last_checked(&tx2.tx_hash, &queued_tx_model.pool, 40)
            .await;

        let transactions = queued_tx_model
            .get_queued_transactions_ready_for_check()
            .await
            .unwrap();

        // both tx should be ready after 40 seconds
        assert_eq!(transactions.len(), 2);
    }

    #[tokio::test]
    async fn test_store_tx_and_increment_retry() {
        let (queued_tx_model, _container) = setup_test_container().await;

        let tx_hash = "test_hash";
        queued_tx_model
            .store_queued_transaction(tx_hash, "test_account", 123)
            .await
            .unwrap();

        let retries: i32 =
            sqlx::query_scalar("SELECT retries FROM xrpl_queued_transactions WHERE tx_hash = $1")
                .bind(tx_hash)
                .fetch_one(&queued_tx_model.pool)
                .await
                .unwrap();

        assert_eq!(retries, 0);

        queued_tx_model.increment_retry(tx_hash).await.unwrap();

        let retries: i32 =
            sqlx::query_scalar("SELECT retries FROM xrpl_queued_transactions WHERE tx_hash = $1")
                .bind(tx_hash)
                .fetch_one(&queued_tx_model.pool)
                .await
                .unwrap();

        assert_eq!(retries, 1);
    }

    #[tokio::test]
    async fn test_update_transaction_status() {
        let (queued_tx_model, _container) = setup_test_container().await;

        let tx1 = QueuedTransaction {
            tx_hash: "test_hash1".to_string(),
            retries: 0,
            account: Some("test_account1".to_string()),
            sequence: Some(123),
            status: QueuedTransactionStatus::Queued,
        };

        let tx2 = QueuedTransaction {
            tx_hash: "test_hash2".to_string(),
            retries: 0,
            account: Some("test_account2".to_string()),
            sequence: Some(124),
            status: QueuedTransactionStatus::Queued,
        };

        let tx3 = QueuedTransaction {
            tx_hash: "test_hash3".to_string(),
            retries: 0,
            account: Some("test_account3".to_string()),
            sequence: Some(125),
            status: QueuedTransactionStatus::Queued,
        };

        let tx4 = QueuedTransaction {
            tx_hash: "test_hash4".to_string(),
            retries: 0,
            account: Some("test_account4".to_string()),
            sequence: Some(126),
            status: QueuedTransactionStatus::Queued,
        };

        queued_tx_model
            .store_queued_transaction(
                tx1.tx_hash.as_str(),
                tx1.account.as_ref().unwrap(),
                tx1.sequence.unwrap(),
            )
            .await
            .unwrap();
        queued_tx_model
            .store_queued_transaction(
                tx2.tx_hash.as_str(),
                tx2.account.as_ref().unwrap(),
                tx2.sequence.unwrap(),
            )
            .await
            .unwrap();
        queued_tx_model
            .store_queued_transaction(
                tx3.tx_hash.as_str(),
                tx3.account.as_ref().unwrap(),
                tx3.sequence.unwrap(),
            )
            .await
            .unwrap();
        queued_tx_model
            .store_queued_transaction(
                tx4.tx_hash.as_str(),
                tx4.account.as_ref().unwrap(),
                tx4.sequence.unwrap(),
            )
            .await
            .unwrap();

        queued_tx_model
            .update_transaction_status(tx1.tx_hash.as_str(), QueuedTransactionStatus::Confirmed)
            .await
            .unwrap();

        queued_tx_model
            .update_transaction_status(tx2.tx_hash.as_str(), QueuedTransactionStatus::Dropped)
            .await
            .unwrap();

        queued_tx_model
            .update_transaction_status(tx3.tx_hash.as_str(), QueuedTransactionStatus::Expired)
            .await
            .unwrap();

        queued_tx_model
            .update_transaction_status(tx4.tx_hash.as_str(), QueuedTransactionStatus::Queued)
            .await
            .unwrap();

        let txs: Vec<QueuedTransaction> =
            sqlx::query_as::<_, QueuedTransaction>("SELECT * FROM xrpl_queued_transactions")
                .fetch_all(&queued_tx_model.pool)
                .await
                .unwrap();

        assert_eq!(txs.len(), 4);
        assert_eq!(txs[0].tx_hash, tx1.tx_hash);
        assert_eq!(txs[0].status, QueuedTransactionStatus::Confirmed);
        assert_eq!(txs[1].tx_hash, tx2.tx_hash);
        assert_eq!(txs[1].status, QueuedTransactionStatus::Dropped);
        assert_eq!(txs[2].tx_hash, tx3.tx_hash);
        assert_eq!(txs[2].status, QueuedTransactionStatus::Expired);
        assert_eq!(txs[3].tx_hash, tx4.tx_hash);
        assert_eq!(txs[3].status, QueuedTransactionStatus::Queued);
    }
}
