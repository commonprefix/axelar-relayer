use crate::gmp_api::gmp_types::{Task};
use sqlx::types::Json;
use sqlx::{PgPool};
use std::future::Future;

const PG_TABLE_NAME: &str = "gmp_tasks";

pub struct TaskModel {
    pub id: String,
    pub chain: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub task_type: String,
    pub message_id: Option<String>,
    pub task: Json<Task>,
    pub _created_at: chrono::DateTime<chrono::Utc>,
    pub _updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl TaskModel {
    pub fn from_task(task: Task) -> Self {
        let db_task = Json(task.clone());

        let (common, message_id, task_value) = match task {
            Task::Execute(t) => {
                let message_id = Some(t.task.message.message_id.clone());
                (t.common.clone(), message_id, serde_json::to_value(t).unwrap())
            },
            Task::Verify(t) => {
                let message_id = Some(t.task.message.message_id.clone());
                (t.common.clone(), message_id, serde_json::to_value(t).unwrap())
            },
            Task::ConstructProof(t) => {
                let message_id = Some(t.task.message.message_id.clone());
                (t.common.clone(), message_id, serde_json::to_value(t).unwrap())
            },
            Task::Refund(t) => {
                let message_id = Some(t.task.message.message_id.clone());
                (t.common.clone(), message_id, serde_json::to_value(t).unwrap())
            },
            Task::GatewayTx(t) => {
                (t.common.clone(), None, serde_json::to_value(t).unwrap())
            },
            Task::ReactToWasmEvent(t) => {
                (t.common.clone(), None, serde_json::to_value(t).unwrap())
            },
            Task::ReactToExpiredSigningSession(t) => {
                (t.common.clone(), None, serde_json::to_value(t).unwrap())
            },
            Task::ReactToRetriablePoll(t) => {
                (t.common.clone(), None, serde_json::to_value(t).unwrap())
            },
            Task::Unknown(t) => {
                (t.common.clone(), None, serde_json::to_value(t).unwrap())
            },
        };

        let timestamp = chrono::DateTime::parse_from_rfc3339(&common.timestamp)
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);

        Self {
            id: common.id,
            chain: common.chain,
            timestamp,
            task_type: common.r#type,
            message_id,
            task: db_task,
            _created_at: chrono::Utc::now(),
            _updated_at: None,
        }
    }

}

#[derive(Debug, Clone)]
pub struct PgGMPTasks {
    pool: PgPool,
}

impl PgGMPTasks {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub trait GMPTaskAudit {
    fn insert_task(&self, task: TaskModel) -> impl Future<Output=anyhow::Result<()>>;
}

impl GMPTaskAudit for PgGMPTasks {
    async fn insert_task(&self, task: TaskModel) -> anyhow::Result<()> {
        let query = format!(
            "INSERT INTO {} (id, chain, timestamp, task_type, message_id, task)
                VALUES ($1, $2, $3, $4, $5, $6)",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(task.id)
            .bind(task.chain)
            .bind(task.timestamp)
            .bind(task.task_type)
            .bind(task.message_id)
            .bind(task.task)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures;
    use crate::gmp_api::gmp_types::Task;
    use sqlx::Row;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres;

    #[test]
    fn test_from_task_execute() {
        let task = fixtures::execute_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "execute_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "EXECUTE");
        assert_eq!(task_model.message_id, Some("message123".to_string()));
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_verify() {
        let task = fixtures::verify_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "verify_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "VERIFY");
        assert_eq!(task_model.message_id, Some("message123".to_string()));
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_construct_proof() {
        let task = fixtures::construct_proof_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "construct_proof_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "CONSTRUCT_PROOF");
        assert_eq!(task_model.message_id, Some("message123".to_string()));
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_refund() {
        let task = fixtures::refund_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "refund_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "REFUND");
        assert_eq!(task_model.message_id, Some("message123".to_string()));
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_gateway_tx() {
        let task = fixtures::gateway_tx_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "gateway_tx_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "GATEWAY_TX");
        assert_eq!(task_model.message_id, None);
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_react_to_wasm_event() {
        let task = fixtures::react_to_wasm_event_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "react_to_wasm_event_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "REACT_TO_WASM_EVENT");
        assert_eq!(task_model.message_id, None);
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_react_to_expired_signing_session() {
        let task = fixtures::react_to_expired_signing_session_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "react_to_expired_signing_session_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "REACT_TO_EXPIRED_SIGNING_SESSION");
        assert_eq!(task_model.message_id, None);
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_react_to_retriable_poll() {
        let task = fixtures::react_to_retriable_poll_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "react_to_retriable_poll_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "REACT_TO_RETRIABLE_POLL");
        assert_eq!(task_model.message_id, None);
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[test]
    fn test_from_task_unknown() {
        let task = fixtures::unknown_task();
        let task_json = task.clone();

        let task_model = TaskModel::from_task(task);

        assert_eq!(task_model.id, "unknown_task_123");
        assert_eq!(task_model.chain, "ton");
        assert_eq!(task_model.task_type, "UNKNOWN");
        assert_eq!(task_model.message_id, None);
        assert_eq!(task_model.task, Json(task_json));

        let expected_timestamp = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .expect("Invalid timestamp format")
            .with_timezone(&chrono::Utc);
        assert_eq!(task_model.timestamp, expected_timestamp);
    }

    #[tokio::test]
    async fn test_crud() {
        let container = postgres::Postgres::default()
            .with_init_sql(
                include_str!("../../../migrations/0012_gmp_audit.sql")
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

        let model = PgGMPTasks::new(pool.clone());

        let task = fixtures::execute_task_with_id("task123");
        let task_model = TaskModel::from_task(task);

        model.insert_task(task_model).await.unwrap();

        let row = sqlx::query("SELECT id, chain, timestamp, task_type, message_id, task, created_at, updated_at FROM gmp_tasks WHERE id = $1")
            .bind("task123")
            .fetch_one(&pool)
            .await
            .unwrap();

        let id: String = row.get("id");
        let chain: String = row.get("chain");
        let task_type: String = row.get("task_type");
        let message_id: String = row.get("message_id");
        assert_eq!(id, "task123");
        assert_eq!(chain, "ton");
        assert_eq!(task_type, "EXECUTE");
        assert_eq!(message_id, "message123".to_string());
    }
}
