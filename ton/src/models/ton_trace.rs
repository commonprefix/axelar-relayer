use relayer_base::models::Model;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::PgPool;
use std::future::Future;
use ton_types::ton_types::{Trace, Transaction};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventSummary {
    pub event_id: String,
    pub message_id: Option<String>,
    pub event_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct TONTrace {
    pub trace_id: String,
    pub is_incomplete: bool,
    pub start_lt: i64,
    pub end_lt: i64,
    pub retries: i32,
    pub transactions: Json<Vec<Transaction>>,
    pub events: Option<Json<Vec<EventSummary>>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl TONTrace {
    pub fn from(trace: &Trace) -> Self {
        Self {
            trace_id: trace.trace_id.to_string(),
            is_incomplete: trace.is_incomplete,
            start_lt: trace.start_lt,
            end_lt: trace.end_lt,
            transactions: Json::from(trace.transactions.clone()),
            events: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
            retries: 5,
        }
    }
}

const PG_TABLE_NAME: &str = "ton_traces";
#[derive(Debug, Clone)]
pub struct PgTONTraceModel {
    pool: PgPool,
}

impl PgTONTraceModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait AtomicUpsert {
    fn upsert_and_return_if_changed(
        &self,
        tx: TONTrace,
    ) -> impl Future<Output = anyhow::Result<Option<TONTrace>>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait Retriable {
    fn fetch_retry(&self, limit: u32)
        -> impl Future<Output = anyhow::Result<Vec<TONTrace>>> + Send;

    fn decrease_retry(&self, tx: TONTrace) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait UpdateEvents {
    fn update_events(
        &self,
        trace_id: String,
        event: Vec<EventSummary>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

impl AtomicUpsert for PgTONTraceModel {
    async fn upsert_and_return_if_changed(&self, tx: TONTrace) -> anyhow::Result<Option<TONTrace>> {
        let query = format!(
            "INSERT INTO {PG_TABLE_NAME} (trace_id, is_incomplete, start_lt, end_lt, transactions)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (trace_id) DO UPDATE
                SET
                    is_incomplete = EXCLUDED.is_incomplete,
                    start_lt = EXCLUDED.start_lt,
                    end_lt = EXCLUDED.end_lt,
                    transactions = EXCLUDED.transactions,
                    updated_at = NOW()
                WHERE
                    ton_traces.is_incomplete IS DISTINCT FROM EXCLUDED.is_incomplete OR
                    ton_traces.start_lt IS DISTINCT FROM EXCLUDED.start_lt OR
                    ton_traces.end_lt IS DISTINCT FROM EXCLUDED.end_lt OR
                    ton_traces.transactions IS DISTINCT FROM EXCLUDED.transactions OR
                    ton_traces.updated_at IS DISTINCT FROM EXCLUDED.updated_at
                RETURNING *;"
        );

        let result = sqlx::query_as::<_, TONTrace>(&query)
            .bind(tx.trace_id)
            .bind(tx.is_incomplete)
            .bind(tx.start_lt)
            .bind(tx.end_lt)
            .bind(tx.transactions)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result)
    }
}

impl Retriable for PgTONTraceModel {
    async fn fetch_retry(&self, limit: u32) -> anyhow::Result<Vec<TONTrace>> {
        let query = format!(
            "SELECT * FROM {PG_TABLE_NAME} WHERE retries > 0 AND is_incomplete = true ORDER BY updated_at NULLS FIRST LIMIT $1"
        );

        let rows = sqlx::query_as::<_, TONTrace>(&query)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows)
    }

    async fn decrease_retry(&self, tx: TONTrace) -> anyhow::Result<()> {
        let query = format!("UPDATE {PG_TABLE_NAME} SET retries = retries - 1 WHERE trace_id = $1");
        sqlx::query(&query)
            .bind(tx.trace_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

impl UpdateEvents for PgTONTraceModel {
    async fn update_events(
        &self,
        trace_id: String,
        events: Vec<EventSummary>,
    ) -> anyhow::Result<()> {
        let query = format!(
            "UPDATE {} SET events = $1, updated_at = NOW() WHERE trace_id = $2",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(Json(events))
            .bind(trace_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

impl Model<TONTrace, String> for PgTONTraceModel {
    async fn upsert(&self, tx: TONTrace) -> anyhow::Result<()> {
        self.upsert_and_return_if_changed(tx).await?;
        Ok(())
    }

    async fn find(&self, id: String) -> anyhow::Result<Option<TONTrace>> {
        let query = format!("SELECT * FROM {PG_TABLE_NAME} WHERE trace_id = $1");
        let tx = sqlx::query_as::<_, TONTrace>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(tx)
    }

    async fn delete(&self, tx: TONTrace) -> anyhow::Result<()> {
        let query = format!("DELETE FROM {PG_TABLE_NAME} WHERE trace_id = $1");
        sqlx::query(&query)
            .bind(tx.trace_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::ton_trace::{
        AtomicUpsert, EventSummary, PgTONTraceModel, TONTrace, UpdateEvents,
    };
    use crate::test_utils::fixtures::fixture_traces;
    use relayer_base::models::Model;
    use sqlx::types::Json;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres;

    #[tokio::test]
    async fn test_crud() {
        let init_sql = format!(
            "{}\n{}",
            include_str!("../../../migrations/0011_ton_traces.sql"),
            include_str!("../../../migrations/0013_ton_traces_events.sql")
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
        let model = PgTONTraceModel::new(pool);
        let transactions = &fixture_traces()[0].transactions;

        let trace = TONTrace {
            trace_id: "123".to_string(),
            is_incomplete: false,
            start_lt: 123,
            end_lt: 321,
            transactions: Json::from(transactions.clone()),
            events: None,
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
            retries: 5,
        };

        let ret = model
            .upsert_and_return_if_changed(trace.clone())
            .await
            .unwrap()
            .unwrap();
        let saved = model.find("123".to_string()).await.unwrap().unwrap();
        assert_eq!(saved.trace_id, "123");
        assert_eq!(saved.transactions[0].hash, "aa1");
        assert_eq!(saved.transactions.len(), transactions.len());
        assert_eq!(saved.start_lt, 123);
        assert_eq!(saved.end_lt, 321);
        assert!(!saved.is_incomplete);

        assert_eq!(ret.trace_id, "123");
        assert_eq!(ret.transactions[0].hash, "aa1");
        assert_eq!(ret.transactions.len(), transactions.len());
        assert_eq!(ret.start_lt, 123);
        assert_eq!(ret.end_lt, 321);
        assert!(!ret.is_incomplete);

        let ret = model
            .upsert_and_return_if_changed(trace.clone())
            .await
            .unwrap();
        assert!(ret.is_none());

        let trace = TONTrace {
            trace_id: "123".to_string(),
            is_incomplete: true,
            start_lt: 123,
            end_lt: 321,
            transactions: Json::from(transactions.clone()),
            events: None,
            created_at: chrono::Utc::now(),
            updated_at: Some(chrono::Utc::now()),
            retries: 5,
        };

        let ret = model
            .upsert_and_return_if_changed(trace.clone())
            .await
            .unwrap();
        assert!(ret.is_some());
        assert!(ret.unwrap().is_incomplete);

        model.delete(trace).await.unwrap();
        let saved = model.find("123".to_string()).await.unwrap();
        assert!(saved.is_none());
    }

    #[tokio::test]
    async fn test_update_events() {
        let init_sql = format!(
            "{}\n{}",
            include_str!("../../../migrations/0011_ton_traces.sql"),
            include_str!("../../../migrations/0013_ton_traces_events.sql")
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
        let model = PgTONTraceModel::new(pool);
        let transactions = &fixture_traces()[0].transactions;

        let trace = TONTrace {
            trace_id: "123".to_string(),
            is_incomplete: false,
            start_lt: 123,
            end_lt: 321,
            transactions: Json::from(transactions.clone()),
            events: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
            retries: 5,
        };

        model.upsert(trace).await.unwrap();

        let events = vec![
            EventSummary {
                event_id: "event1".to_string(),
                message_id: Some("message1".to_string()),
                event_type: "GAS_REFUNDED".to_string(),
            },
            EventSummary {
                event_id: "event2".to_string(),
                message_id: Some("message2".to_string()),
                event_type: "CANNOT_EXECUTE_MESSAGE_V2".to_string(),
            },
        ];

        model
            .update_events("123".to_string(), events.clone())
            .await
            .unwrap();

        let updated_trace = model.find("123".to_string()).await.unwrap().unwrap();
        assert!(updated_trace.events.is_some());

        let updated_events = updated_trace.events.unwrap();
        assert_eq!(updated_events.len(), 2);
        assert_eq!(updated_events[0].event_id, "event1");
        assert_eq!(updated_events[0].message_id, Some("message1".to_string()));
        assert_eq!(updated_events[0].event_type, "GAS_REFUNDED");
        assert_eq!(updated_events[1].event_id, "event2");
        assert_eq!(updated_events[1].message_id, Some("message2".to_string()));
        assert_eq!(updated_events[1].event_type, "CANNOT_EXECUTE_MESSAGE_V2");

        assert!(updated_trace.updated_at.is_some());
    }
}
