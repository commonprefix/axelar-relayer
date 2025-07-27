/*! # GMP API Database Audit Decorator

Decorator pattern for the GMP API that adds database auditing.
The `GmpApiDbAuditDecorator` wraps a GMP API implementation and adds database logging.

It's best to use the convenience function `construct_gmp_api` to create a `GmpApiDbAuditDecorator`.

Otherwise, you can do it like so:

# Example

```rust
use relayer_base::gmp_api::{GmpApiDbAuditDecorator, GmpApiTrait, GmpApi, construct_gmp_api};
use relayer_base::models::gmp_events::{GMPAudit, PgGMPEvents};
use relayer_base::models::gmp_tasks::{GMPTaskAudit, PgGMPTasks};
use relayer_base::config::Config;
use sqlx::PgPool;
use std::sync::Arc;

async fn create(config: &Config, pg_pool: PgPool) -> anyhow::Result<impl GmpApiTrait> {
    // Create the GMP API
    let gmp_api = GmpApi::new(config, true)?;

    // Create both models using the provided PgPool
    let gmp_tasks = PgGMPTasks::new(pg_pool.clone());
    let gmp_events = PgGMPEvents::new(pg_pool);

    // Create a decorated GMP API with database auditing
    let decorated_api = GmpApiDbAuditDecorator::new(gmp_api, gmp_tasks, gmp_events);

    // Now you can use decorated_api as a regular GmpApiTrait implementation
    // with automatic database auditing for tasks and events 
    Ok(decorated_api)
}
```
*/

use crate::error::GmpApiError;
use crate::gmp_api::gmp_types::{
    BroadcastRequest, CannotExecuteMessageReason, Event, PostEventResult, QueryRequest, Task,
};
use crate::gmp_api::{GmpApi, GmpApiTrait};
use crate::models::gmp_events::{EventModel, GMPAudit, PgGMPEvents};
use crate::models::gmp_tasks::{GMPTaskAudit, TaskModel, PgGMPTasks};
use crate::config::Config;
use sqlx::{types::Json, PgPool};
use std::sync::Arc;
use tracing::error;
use xrpl_amplifier_types::msg::XRPLMessage;

pub struct GmpApiDbAuditDecorator<T: GmpApiTrait, U: GMPTaskAudit, V: GMPAudit> {
    gmp_api: T,
    gmp_tasks: U,
    gmp_events: V,
}

impl<T: GmpApiTrait, U: GMPTaskAudit, V: GMPAudit> GmpApiDbAuditDecorator<T, U, V> {
    pub fn new(gmp_api: T, gmp_tasks: U, gmp_events: V) -> Self {
        Self {
            gmp_api,
            gmp_tasks,
            gmp_events,
        }
    }
}

/// Constructs a GmpApiDbAuditDecorator with GmpApi, PgGMPTasks, and PgGMPEvents
///
/// # Arguments
/// 
/// * `pg_pool` - A PostgreSQL connection pool
/// * `config` - The configuration for the GmpApi
/// * `connection_pooling` - Whether to enable connection pooling for the GmpApi
/// 
/// # Returns
/// 
/// A Result containing an Arc-wrapped GmpApiDbAuditDecorator or a GmpApiError
pub fn construct_gmp_api(
    pg_pool: PgPool,
    config: &Config,
    connection_pooling: bool,
) -> Result<Arc<GmpApiDbAuditDecorator<GmpApi, PgGMPTasks, PgGMPEvents>>, GmpApiError> {
    let gmp_tasks = PgGMPTasks::new(pg_pool.clone());
    let gmp_events = PgGMPEvents::new(pg_pool);

    let gmp_api_base = GmpApi::new(config, connection_pooling)?;
    let gmp_api = Arc::new(GmpApiDbAuditDecorator::new(
        gmp_api_base,
        gmp_tasks,
        gmp_events,
    ));

    Ok(gmp_api)
}

impl<T: GmpApiTrait + Send + Sync, U: GMPTaskAudit + Send + Sync, V: GMPAudit + Send + Sync>
    GmpApiTrait for GmpApiDbAuditDecorator<T, U, V>
{
    fn get_chain(&self) -> &str {
        self.gmp_api.get_chain()
    }

    async fn get_tasks_action(&self, after: Option<String>) -> Result<Vec<Task>, GmpApiError> {
        let tasks = self.gmp_api.get_tasks_action(after).await?;

        for task in &tasks {
            let task_model = TaskModel::from_task(task.clone());
            if let Err(e) = self.gmp_tasks.insert_task(task_model).await {
                error!("Failed to save task to database: {:?}", e);
            }
        }

        Ok(tasks)
    }

    async fn post_events(&self, events: Vec<Event>) -> Result<Vec<PostEventResult>, GmpApiError> {
        let mut event_models = Vec::new();
        for event in &events {
            let event_model = EventModel::from_event(event.clone());
            event_models.push(event_model.clone());
            if let Err(e) = self.gmp_events.insert_event(event_model).await {
                error!("Failed to save event to database: {:?}", e);
            }
        }

        let results = self.gmp_api.post_events(events).await?;

        for (i, result) in results.iter().enumerate() {
            if i < event_models.len() {
                let event_id = event_models[i].event_id.clone();
                if let Err(e) = self
                    .gmp_events
                    .update_event_response(event_id, Json(result.clone()))
                    .await
                {
                    error!("Failed to update event response in database: {:?}", e);
                }
            }
        }

        Ok(results)
    }

    async fn post_broadcast(
        &self,
        contract_address: String,
        data: &BroadcastRequest,
    ) -> Result<String, GmpApiError> {
        self.gmp_api.post_broadcast(contract_address, data).await
    }

    async fn get_broadcast_result(
        &self,
        contract_address: String,
        broadcast_id: String,
    ) -> Result<String, GmpApiError> {
        self.gmp_api
            .get_broadcast_result(contract_address, broadcast_id)
            .await
    }

    async fn post_query(
        &self,
        contract_address: String,
        data: &QueryRequest,
    ) -> Result<String, GmpApiError> {
        self.gmp_api.post_query(contract_address, data).await
    }

    async fn post_payload(&self, payload: &[u8]) -> Result<String, GmpApiError> {
        self.gmp_api.post_payload(payload).await
    }

    async fn get_payload(&self, hash: &str) -> Result<String, GmpApiError> {
        self.gmp_api.get_payload(hash).await
    }

    async fn cannot_execute_message(
        &self,
        id: String,
        message_id: String,
        source_chain: String,
        details: String,
        reason: CannotExecuteMessageReason,
    ) -> Result<(), GmpApiError> {
        self.gmp_api
            .cannot_execute_message(id, message_id, source_chain, details, reason)
            .await
    }

    async fn its_interchain_transfer(&self, xrpl_message: XRPLMessage) -> Result<(), GmpApiError> {
        self.gmp_api.its_interchain_transfer(xrpl_message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures;
    use mockall::predicate::*;
    #[tokio::test]
    async fn test_get_tasks_action() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mut mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        let tasks = vec![fixtures::execute_task(), fixtures::verify_task()];
        let tasks_clone = tasks.clone();

        mock_gmp_api.expect_get_tasks_action().returning(move |_| {
            let tasks = tasks_clone.clone();
            Box::pin(async move { Ok(tasks) })
        });

        mock_gmp_tasks
            .expect_insert_task()
            .returning(|_| Box::pin(async { Ok(()) }));

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.get_tasks_action(None).await;

        assert!(result.is_ok());
        let returned_tasks = result.unwrap();
        assert_eq!(returned_tasks.len(), 2);

        match &returned_tasks[0] {
            Task::Execute(_) => {}
            _ => panic!("Expected Execute task"),
        }
        match &returned_tasks[1] {
            Task::Verify(_) => {}
            _ => panic!("Expected Verify task"),
        }
    }

    #[tokio::test]
    async fn test_get_tasks_action_with_after() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mut mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        let after = Some("last_task_id".to_string());
        let tasks = vec![fixtures::gateway_tx_task()];
        let tasks_clone = tasks.clone();

        mock_gmp_api
            .expect_get_tasks_action()
            .with(eq(after.clone()))
            .returning(move |_| {
                let tasks = tasks_clone.clone();
                Box::pin(async move { Ok(tasks) })
            });

        mock_gmp_tasks
            .expect_insert_task()
            .returning(|_| Box::pin(async { Ok(()) }));

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.get_tasks_action(after).await;

        assert!(result.is_ok());
        let returned_tasks = result.unwrap();
        assert_eq!(returned_tasks.len(), 1);

        match &returned_tasks[0] {
            Task::GatewayTx(_) => {}
            _ => panic!("Expected GatewayTx task"),
        }
    }

    #[tokio::test]
    async fn test_get_tasks_action_error_handling() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        mock_gmp_api
            .expect_get_tasks_action()
            .with(eq(None))
            .returning(|_| {
                Box::pin(async { Err(GmpApiError::RequestFailed("API error".to_string())) })
            });

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.get_tasks_action(None).await;

        assert!(result.is_err());
        match result {
            Err(GmpApiError::RequestFailed(msg)) => {
                assert_eq!(msg, "API error");
            }
            _ => panic!("Expected RequestFailed error"),
        }
    }

    #[tokio::test]
    async fn test_get_tasks_action_db_error_handling() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mut mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        let tasks = vec![fixtures::execute_task()];
        let tasks_clone = tasks.clone();

        mock_gmp_api
            .expect_get_tasks_action()
            .with(eq(None))
            .returning(move |_| {
                let tasks = tasks_clone.clone();
                Box::pin(async move { Ok(tasks) })
            });

        mock_gmp_tasks
            .expect_insert_task()
            .returning(|_| Box::pin(async { Err(anyhow::anyhow!("Database error")) }));

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.get_tasks_action(None).await;

        assert!(result.is_ok());
        let returned_tasks = result.unwrap();
        assert_eq!(returned_tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_post_events() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mut mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        let events = vec![
            fixtures::gas_refunded_event(),
            fixtures::message_executed_event(),
        ];

        let results = vec![
            PostEventResult {
                status: "success".to_string(),
                index: 0,
                error: None,
                retriable: None,
            },
            PostEventResult {
                status: "success".to_string(),
                index: 1,
                error: None,
                retriable: None,
            },
        ];
        let results_clone = results.clone();

        mock_gmp_events
            .expect_insert_event()
            .returning(|_| Box::pin(async { Ok(()) }));

        mock_gmp_api.expect_post_events().returning(move |_| {
            let results = results_clone.clone();
            Box::pin(async move { Ok(results) })
        });

        for (i, event) in events.iter().enumerate() {
            let event_model = EventModel::from_event(event.clone());
            let result = results[i].clone();
            mock_gmp_events
                .expect_update_event_response()
                .with(eq(event_model.event_id), eq(Json(result)))
                .returning(|_, _| Box::pin(async { Ok(()) }));
        }

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.post_events(events).await;

        assert!(result.is_ok());
        let returned_results = result.unwrap();
        assert_eq!(returned_results.len(), 2);
        assert_eq!(returned_results[0].status, "success");
        assert_eq!(returned_results[1].status, "success");
    }

    #[tokio::test]
    async fn test_post_events_api_error() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mut mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        let events = vec![fixtures::gas_refunded_event()];

        mock_gmp_events
            .expect_insert_event()
            .returning(|_| Box::pin(async { Ok(()) }));

        mock_gmp_api.expect_post_events().returning(|_| {
            Box::pin(async { Err(GmpApiError::RequestFailed("API error".to_string())) })
        });

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.post_events(events).await;

        assert!(result.is_err());
        match result {
            Err(GmpApiError::RequestFailed(msg)) => {
                assert_eq!(msg, "API error");
            }
            _ => panic!("Expected RequestFailed error"),
        }
    }

    #[tokio::test]
    async fn test_post_events_db_error_handling() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mut mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        let events = vec![fixtures::gas_refunded_event()];

        let results = vec![PostEventResult {
            status: "success".to_string(),
            index: 0,
            error: None,
            retriable: None,
        }];
        let results_clone = results.clone();

        mock_gmp_events
            .expect_insert_event()
            .returning(|_| Box::pin(async { Err(anyhow::anyhow!("Database error")) }));

        mock_gmp_api.expect_post_events().returning(move |_| {
            let results = results_clone.clone();
            Box::pin(async move { Ok(results) })
        });

        mock_gmp_events
            .expect_update_event_response()
            .returning(|_, _| Box::pin(async { Err(anyhow::anyhow!("Database error")) }));

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let result = decorator.post_events(events).await;

        assert!(result.is_ok());
        let returned_results = result.unwrap();
        assert_eq!(returned_results.len(), 1);
        assert_eq!(returned_results[0].status, "success");
    }

    #[tokio::test]
    async fn test_delegation_methods() {
        let mut mock_gmp_api = crate::gmp_api::MockGmpApiTrait::new();
        let mock_gmp_tasks = crate::models::gmp_tasks::MockGMPTaskAudit::new();
        let mock_gmp_events = crate::models::gmp_events::MockGMPAudit::new();

        mock_gmp_api
            .expect_post_broadcast()
            .with(eq("contract123".to_string()), always())
            .returning(|_, _| Box::pin(async { Ok("tx_hash".to_string()) }));

        mock_gmp_api
            .expect_get_broadcast_result()
            .with(
                eq("contract123".to_string()),
                eq("broadcast123".to_string()),
            )
            .returning(|_, _| Box::pin(async { Ok("tx_hash".to_string()) }));

        mock_gmp_api
            .expect_post_query()
            .with(eq("contract123".to_string()), always())
            .returning(|_, _| Box::pin(async { Ok("query_result".to_string()) }));

        mock_gmp_api
            .expect_post_payload()
            .with(always())
            .returning(|_| Box::pin(async { Ok("payload_hash".to_string()) }));

        mock_gmp_api
            .expect_get_payload()
            .with(eq("hash123"))
            .returning(|_| Box::pin(async { Ok("payload_data".to_string()) }));

        mock_gmp_api
            .expect_cannot_execute_message()
            .with(
                eq("id123".to_string()),
                eq("message123".to_string()),
                eq("source123".to_string()),
                eq("details123".to_string()),
                eq(CannotExecuteMessageReason::InsufficientGas),
            )
            .returning(|_, _, _, _, _| Box::pin(async { Ok(()) }));

        mock_gmp_api
            .expect_its_interchain_transfer()
            .with(always())
            .returning(|_| Box::pin(async { Ok(()) }));

        let decorator = GmpApiDbAuditDecorator::new(mock_gmp_api, mock_gmp_tasks, mock_gmp_events);

        let broadcast_request = crate::gmp_api::gmp_types::BroadcastRequest::Generic(
            serde_json::json!({"data": "test"}),
        );
        let result = decorator
            .post_broadcast("contract123".to_string(), &broadcast_request)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "tx_hash");

        let result = decorator
            .get_broadcast_result("contract123".to_string(), "broadcast123".to_string())
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "tx_hash");

        let query_request =
            crate::gmp_api::gmp_types::QueryRequest::Generic(serde_json::json!({"query": "test"}));
        let result = decorator
            .post_query("contract123".to_string(), &query_request)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "query_result");

        let result = decorator.post_payload(b"test_payload").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "payload_hash");

        let result = decorator.get_payload("hash123").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "payload_data");

        let result = decorator
            .cannot_execute_message(
                "id123".to_string(),
                "message123".to_string(),
                "source123".to_string(),
                "details123".to_string(),
                CannotExecuteMessageReason::InsufficientGas,
            )
            .await;
        assert!(result.is_ok());
    }
}
