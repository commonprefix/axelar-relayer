use crate::gmp_api::gmp_types::{Event, PostEventResult};
use sqlx::types::Json;
use sqlx::{PgPool};
use std::future::Future;

const PG_TABLE_NAME: &str = "gmp_events";

pub struct EventModel {
    pub id: String,
    pub message_id: Option<String>,
    pub event_type: String,
    pub event: Json<Event>,
    pub _response: Option<Json<PostEventResult>>,
    pub _created_at: chrono::DateTime<chrono::Utc>,
    pub _updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl EventModel {
    pub fn from_event(event: Event) -> Self {
        let (id, event_type, message_id) = match &event {
            Event::GasRefunded { common, message_id, .. }
            | Event::GasCredit { common, message_id, .. }
            | Event::CannotExecuteMessageV2 { common, message_id, .. }
            | Event::ITSInterchainTransfer { common, message_id, .. } => (
                common.event_id.clone(),
                common.r#type.clone(),
                Some(message_id.clone()),
            ),
            Event::MessageExecuted { common, message_id, .. } => (
                common.event_id.clone(),
                common.r#type.clone(),
                Some(message_id.clone()),
            ),
            Event::Call { common, message, .. } => (
                common.event_id.clone(),
                common.r#type.clone(),
                Some(message.message_id.clone()),
            ),
            Event::MessageApproved { common, message, .. } => (
                common.event_id.clone(),
                common.r#type.clone(),
                Some(message.message_id.clone()),
            ),
        };

        Self {
            id,
            message_id,
            event_type,
            event: Json(event),
            _response: None,
            _created_at: chrono::Utc::now(),
            _updated_at: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PgGMPEvents {
    pool: PgPool,
}

impl PgGMPEvents {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub trait GMPAudit {
    fn insert_event(&self, event: EventModel) -> impl Future<Output=anyhow::Result<()>>;
    fn update_event_response(&self, event_id: String, response: Json<PostEventResult>) -> impl Future<Output=anyhow::Result<()>>;
}

impl GMPAudit for PgGMPEvents {
    async fn insert_event(&self, event: EventModel) -> anyhow::Result<()> {
        let query = format!(
            "INSERT INTO {} (id, message_id, event_type, event)
                VALUES ($1, $2, $3, $4)",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(event.id)
            .bind(event.message_id)
            .bind(event.event_type)
            .bind(event.event)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn update_event_response(&self, id: String, response: Json<PostEventResult>) -> anyhow::Result<()> {
        let query = format!(
            "UPDATE {} SET response = $1, updated_at = NOW() WHERE id = $2",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(response)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures;
    use sqlx::Row;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres;

    #[test]
    fn test_from_event_gas_refunded() {
        let event = fixtures::gas_refunded_event();
        let event_id = "event123".to_string();
        let event_type = "GAS_REFUNDED".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[test]
    fn test_from_event_gas_credit() {
        let event = fixtures::gas_credit_event();
        let event_id = "event123".to_string();
        let event_type = "GAS_CREDIT".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[test]
    fn test_from_event_cannot_execute_message_v2() {
        let event = fixtures::cannot_execute_message_v2_event();
        let event_id = "event123".to_string();
        let event_type = "CANNOT_EXECUTE_MESSAGE_V2".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[test]
    fn test_from_event_its_interchain_transfer() {
        let event = fixtures::its_interchain_transfer_event();
        let event_id = "event123".to_string();
        let event_type = "ITS_INTERCHAIN_TRANSFER".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[test]
    fn test_from_event_message_executed() {
        let event = fixtures::message_executed_event();
        let event_id = "event123".to_string();
        let event_type = "MESSAGE_EXECUTED".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[test]
    fn test_from_event_call() {
        let event = fixtures::call_event();
        let event_id = "event123".to_string();
        let event_type = "CALL".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[test]
    fn test_from_event_message_approved() {
        let event = fixtures::message_approved_event();
        let event_id = "event123".to_string();
        let event_type = "MESSAGE_APPROVED".to_string();
        let message_id = "message123".to_string();

        let model = EventModel::from_event(event);

        assert_eq!(model.id, event_id);
        assert_eq!(model.event_type, event_type);
        assert_eq!(model.message_id, Some(message_id));
    }

    #[tokio::test]
    #[ignore]
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


        let model = PgGMPEvents::new(pool.clone());

        let event = fixtures::gas_refunded_event();
        let event_model = EventModel::from_event(event.clone());

        model.insert_event(event_model).await.unwrap();

        let row = sqlx::query("SELECT id, message_id, event_type FROM gmp_events WHERE id = $1")
            .bind("event123")
            .fetch_one(&pool)
            .await
            .unwrap();

        let id: String = row.get("id");
        let message_id: String = row.get("message_id");
        let event_type: String = row.get("event_type");

        assert_eq!(id, "event123");
        assert_eq!(message_id, "message123");
        assert_eq!(event_type, "GAS_REFUNDED");

        let response = PostEventResult {
            status: "success".to_string(),
            index: 0,
            error: None,
            retriable: None,
        };

        model.update_event_response("event123".to_string(), Json(response.clone())).await.unwrap();

        let row = sqlx::query("SELECT response FROM gmp_events WHERE id = $1")
            .bind("event123")
            .fetch_one(&pool)
            .await
            .unwrap();

        let db_response: serde_json::Value = row.get("response");
        let expected_response = serde_json::to_value(response).unwrap();

        assert_eq!(db_response, expected_response);
    }
}
