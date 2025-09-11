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
    pub ixs: Vec<String>,
    pub events: Vec<String>,
    pub cost_units: i64,
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
             (signature, slot, logs, ixs, events, cost_units, retries, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) \
             ON CONFLICT (signature) DO NOTHING \
            ",
            PG_TABLE_NAME
        );

        let res = sqlx::query(&query)
            .bind(tx.signature)
            .bind(tx.slot)
            .bind(tx.logs)
            .bind(tx.ixs)
            .bind(tx.events)
            .bind(tx.cost_units)
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use solana_sdk::signature::Signature;
    use solana_transaction_status::UiInnerInstructions;
    use solana_types::solana_types::SolanaTransaction;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres;

    use crate::solana_transaction::{
        EventSummary, PgSolanaTransactionModel, SolanaTransactionData, SolanaTransactionModel,
        UpdateEvents,
    };
    use crate::test_utils::fixtures::fixtures;

    #[tokio::test]
    async fn test_crud() {
        let init_sql = format!(
            "{}\n",
            include_str!("../../../migrations/0014_solana_transactions.sql"),
        );
        let container = postgres::Postgres::default()
            .with_init_sql(init_sql.into_bytes())
            .start()
            .await
            .unwrap();
        let connection_string = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await.unwrap(),
            container.get_host_port_ipv4(5432).await.unwrap()
        );
        let pool = sqlx::PgPool::connect(&connection_string).await.unwrap();
        let model = PgSolanaTransactionModel::new(pool);
        let solana_tx = &fixtures()[0];

        let tx = SolanaTransactionData {
            signature: solana_tx.signature.to_string(),
            slot: solana_tx.slot,
            logs: solana_tx.logs.clone(),
            ixs: solana_tx
                .ixs
                .clone()
                .iter()
                .map(|ix| serde_json::to_string(ix).unwrap())
                .collect::<Vec<String>>(),
            events: Vec::new(),
            cost_units: solana_tx.cost_units as i64,
            retries: 3,
            created_at: None,
        };

        let ret = model.upsert(tx.clone()).await.unwrap();
        let saved = model
            .find(solana_tx.signature.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(ret);

        let tx_from_data = SolanaTransaction {
            signature: Signature::from_str(&saved.signature).unwrap(),
            timestamp: solana_tx.timestamp,
            slot: saved.slot,
            logs: saved.logs,
            ixs: saved
                .ixs
                .iter()
                .map(|ix| serde_json::from_str(ix).unwrap())
                .collect::<Vec<UiInnerInstructions>>(),
            cost_units: saved.cost_units as u64,
        };

        assert_eq!(tx_from_data, *solana_tx);

        // test that if you upsert when signature exists already, there are no changes
        let ret = model.upsert(tx.clone()).await.unwrap();
        assert!(!ret);

        let events = vec![EventSummary {
            event_id: "123".to_string(),
            message_id: None,
            event_type: "123".to_string(),
        }];
        model
            .update_events(tx.signature.clone(), events.clone())
            .await
            .unwrap();
        let saved = model.find(tx.signature.clone()).await.unwrap().unwrap();
        assert_eq!(
            saved.events,
            events
                .iter()
                .map(|e| e.event_id.clone())
                .collect::<Vec<String>>()
        );

        model.delete(tx).await.unwrap();
        let saved = model.find(solana_tx.signature.to_string()).await.unwrap();
        assert!(saved.is_none());
    }
}
