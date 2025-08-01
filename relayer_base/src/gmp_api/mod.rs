pub mod gmp_api_db_audit_decorator;
pub mod gmp_types;
pub use gmp_api_db_audit_decorator::construct_gmp_api;
pub use gmp_api_db_audit_decorator::GmpApiDbAuditDecorator;

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::future::Future;
use std::{
    collections::HashMap,
    fs::{self},
    path::PathBuf,
    time::Duration,
};
use opentelemetry::{global, Context, KeyValue};
use opentelemetry::trace::{FutureExt, Span, Tracer};
use tracing::{debug, info, warn};
use xrpl_amplifier_types::msg::XRPLMessage;

use reqwest::Identity;
use reqwest_tracing::TracingMiddleware;
use tracing::log::error;
use crate::{config::Config, error::GmpApiError, utils::parse_task};
use gmp_types::{
    Amount, BroadcastRequest, CannotExecuteMessageReason, CommonEventFields, Event,
    PostEventResponse, PostEventResult, QueryRequest, StorePayloadResult, Task,
};

const MAX_BROADCAST_WAIT_TIME_SECONDS: u32 = 60; // 60 seconds
const BROADCAST_POLL_INTERVAL_SECONDS: u32 = 2; // 2 seconds

pub struct GmpApi {
    rpc_url: String,
    client: ClientWithMiddleware,
    pub chain: String,
}


fn identity_from_config(config: &Config) -> Result<Identity, GmpApiError> {
    let base_path = std::env::var("BASE_PATH").ok();

    let project_root = if let Some(path) = base_path {
        PathBuf::from(path)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    };

    let key_path = project_root.join(&config.client_key_path);
    let cert_path = project_root.join(&config.client_cert_path);

    let key = fs::read(&key_path).map_err(|e| {
        GmpApiError::GenericError(format!(
            "Failed to read client key from {:?}: {}",
            key_path, e
        ))
    })?;

    let cert = fs::read(&cert_path).map_err(|e| {
        GmpApiError::GenericError(format!(
            "Failed to read client certificate from {:?}: {}",
            cert_path, e
        ))
    })?;

    Identity::from_pkcs8_pem(&cert, &key).map_err(|e| {
        GmpApiError::GenericError(format!(
            "Failed to create identity from certificate and key: {}",
            e
        ))
    })
}

impl GmpApi {
    pub fn new(config: &Config, connection_pooling: bool) -> Result<Self, GmpApiError> {
        let mut client_builder = reqwest::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .identity(identity_from_config(config)?);
        if connection_pooling {
            client_builder = client_builder.pool_idle_timeout(Some(Duration::from_secs(300)));
        } else {
            client_builder = client_builder.pool_max_idle_per_host(0);
        }

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(
            client_builder
                .build()
                .map_err(|e| GmpApiError::ConnectionFailed(e.to_string()))?,
        )
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .with(TracingMiddleware::default())
        .build();

        Ok(Self {
            rpc_url: config.gmp_api_url.to_owned(),
            client,
            chain: config.chain_name.to_owned(),
        })
    }

    async fn request_bytes_if_success(
        request: reqwest_middleware::RequestBuilder,
    ) -> Result<Vec<u8>, GmpApiError> {
        let response = request
            .send()
            .await
            .map_err(|e| GmpApiError::RequestFailed(e.to_string()))?;

        if response.status().is_success() {
            response
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| GmpApiError::InvalidResponse(e.to_string()))
        } else {
            Err(GmpApiError::ErrorResponse(
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error body".to_string()),
            ))
        }
    }

    async fn request_text_if_success(
        request: reqwest_middleware::RequestBuilder,
    ) -> Result<String, GmpApiError> {
        let response = request
            .send()
            .await
            .map_err(|e| GmpApiError::RequestFailed(e.to_string()))?;

        // Convert any non-200 status to an error, otherwise retrieve the response body.
        if response.status().is_success() {
            response
                .text()
                .await
                .map_err(|e| GmpApiError::InvalidResponse(e.to_string()))
        } else {
            Err(GmpApiError::ErrorResponse(
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error body".to_string()),
            ))
        }
    }

    async fn request_json<T: DeserializeOwned>(
        request: reqwest_middleware::RequestBuilder,
    ) -> Result<T, GmpApiError> {
        let tracer = global::tracer("gmp_api");

        let span_name;
        // TODO: Move this ideally to a middleware (existing reqwest-middleware for tracing doesn't cut it)
        if let Some(cloned) = request.try_clone() {
            match cloned.build() {
                Ok(request) => {
                    span_name = format!("{} {}", request.method().to_string().to_uppercase(), request.url());
                },
                Err(e) => {
                    error!("Failed to build request: {}", e);
                    span_name = "UNKNOWN".to_string();
                }
            }
        } else {
            error!("Failed to clone RequestBuilder");
            span_name = "UNKNOWN".to_string();
        }
        let _span = tracer.start_with_context(span_name, &Context::current());

        let response = request.send().await.map_err(|e| {
            debug!("{:?}", e);
            GmpApiError::RequestFailed(e.to_string())
        })?;

        response
            .error_for_status_ref()
            .map_err(|e| GmpApiError::ErrorResponse(e.to_string()))?;

        response
            .json::<T>()
            .await
            .map_err(|e| GmpApiError::InvalidResponse(e.to_string()))
    }
}

#[cfg_attr(test, mockall::automock)]
impl GmpApiTrait for GmpApi {
    fn get_chain(&self) -> &str {
        &self.chain
    }

    async fn get_tasks_action(&self, after: Option<String>) -> Result<Vec<Task>, GmpApiError> {
        let request_url = format!("{}/chains/{}/tasks", self.rpc_url, self.chain);
        let mut request = self.client.get(&request_url);

        if let Some(after) = after {
            request = request.query(&[("after", &after)]);
            debug!("Requesting tasks after: {}", after);
        }

        let response: HashMap<String, Vec<Value>> = GmpApi::request_json(request).with_current_context().await?;
        debug!("Response from {}: {:?}", request_url, response);

        let tasks_json = response
            .get("tasks")
            .ok_or_else(|| GmpApiError::InvalidResponse("Missing 'tasks' field".to_string()))?;

        Ok(tasks_json
            .iter()
            .filter_map(|task_json| match parse_task(task_json) {
                Ok(task) => Some(task),
                Err(e) => {
                    warn!("Failed to parse task: {:?}", e);
                    None
                }
            })
            .collect::<Vec<_>>())
    }
    async fn post_events(&self, events: Vec<Event>) -> Result<Vec<PostEventResult>, GmpApiError> {
        let mut map = HashMap::new();
        map.insert("events", events);

        debug!("Posting events: {:?}", map);

        let url = format!("{}/chains/{}/events", self.rpc_url, self.chain);

        let request = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&map).map_err(|e| {
                GmpApiError::GenericError(format!("Failed to serialize events payload: {}", e))
            })?);

        let response: PostEventResponse = GmpApi::request_json(request).with_current_context().await?;
        info!("Response from POST: {:?}", response);
        Ok(response.results)
    }
    async fn post_broadcast(
        &self,
        contract_address: String,
        data: &BroadcastRequest,
    ) -> Result<String, GmpApiError> {
        let url = format!("{}/contracts/{}/broadcasts", self.rpc_url, contract_address);

        let BroadcastRequest::Generic(payload) = data;

        debug!("Broadcast:");
        debug!("URL: {}", url);
        debug!(
            "Payload: {}",
            serde_json::to_string(payload).map_err(|e| {
                GmpApiError::GenericError(format!("Failed to serialize broadcast payload: {}", e))
            })?
        );

        let request = self
            .client
            .post(url.clone())
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(payload).map_err(|e| {
                GmpApiError::GenericError(format!("Failed to serialize broadcast payload: {}", e))
            })?);

        let response = GmpApi::request_text_if_success(request).await?;
        debug!("Broadcast successful: {:?}", response);

        let response_json: serde_json::Value = serde_json::from_str(&response).map_err(|e| {
            GmpApiError::GenericError(format!("Failed to parse GMP response {}: {}", response, e))
        })?;

        let broadcast_id = response_json
            .get("broadcastID")
            .ok_or_else(|| GmpApiError::GenericError("Missing 'broadcastID' field".to_string()))?
            .as_str()
            .ok_or_else(|| {
                GmpApiError::GenericError("'broadcastID' field is not a string".to_string())
            })?;

        let broadcast_result = self
            .get_broadcast_result(contract_address, broadcast_id.to_string())
            .await;

        if matches!(&broadcast_result, Err(GmpApiError::Timeout(_))) {
            info!("Broadcast id {} timed out.", broadcast_id);
            return Ok("timeout".to_string());
        }

        debug!(
            "Broadcast id {} result: {:?}",
            broadcast_id, broadcast_result
        );

        broadcast_result
    }
    async fn get_broadcast_result(
        &self,
        contract_address: String,
        broadcast_id: String,
    ) -> Result<String, GmpApiError> {
        let url = format!(
            "{}/contracts/{}/broadcasts/{}",
            self.rpc_url, contract_address, broadcast_id
        );
        let mut retries = 0;
        loop {
            if retries > (MAX_BROADCAST_WAIT_TIME_SECONDS / BROADCAST_POLL_INTERVAL_SECONDS) {
                return Err(GmpApiError::Timeout(format!(
                    "Broadcast with id {} timed out",
                    broadcast_id
                )));
            }
            retries += 1;

            let request = self.client.get(&url);
            let response: Value = GmpApi::request_json(request).await?;

            match response.get("status") {
                Some(status) => match status.as_str() {
                    Some("RECEIVED") => {
                        tokio::time::sleep(Duration::from_secs(
                            BROADCAST_POLL_INTERVAL_SECONDS as u64,
                        ))
                        .await;
                        continue;
                    }
                    Some("SUCCESS") => {
                        let tx_hash = response
                            .get("txHash")
                            .ok_or_else(|| {
                                GmpApiError::GenericError(
                                    "Broadcast result missing 'txHash' field".to_string(),
                                )
                            })?
                            .as_str()
                            .ok_or_else(|| {
                                GmpApiError::GenericError(
                                    "'txHash' field is not a string".to_string(),
                                )
                            })?;

                        return Ok(tx_hash.to_string());
                    }
                    _ => {
                        if let Some(error) = response.get("error") {
                            return Err(GmpApiError::GenericError(error.to_string()));
                        } else {
                            return Err(GmpApiError::GenericError(
                                response
                                    .get("status")
                                    .ok_or_else(|| {
                                        GmpApiError::GenericError(
                                            "Broadcast result missing 'status' field".to_string(),
                                        )
                                    })?
                                    .to_string(),
                            ));
                        }
                    }
                },
                None => match response.get("error") {
                    Some(error) => {
                        return Err(GmpApiError::GenericError(error.to_string()));
                    }
                    None => {
                        let string_response = serde_json::to_string(&response).map_err(|e| {
                            GmpApiError::GenericError(format!(
                                "Failed to parse broadcast result {:?}: {}",
                                response, e
                            ))
                        })?;

                        return Err(GmpApiError::GenericError(format!(
                            "Broadcast failed: {}",
                            string_response
                        )));
                    }
                },
            }
        }
    }
    async fn post_query(
        &self,
        contract_address: String,
        data: &QueryRequest,
    ) -> Result<String, GmpApiError> {
        let url = format!("{}/contracts/{}/queries", self.rpc_url, contract_address);

        let QueryRequest::Generic(payload) = data;

        let request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(payload).map_err(|e| {
                GmpApiError::GenericError(format!("Failed to serialize query payload: {}", e))
            })?);

        GmpApi::request_text_if_success(request).await
    }
    async fn post_payload(&self, payload: &[u8]) -> Result<String, GmpApiError> {
        let url = format!("{}/payloads", self.rpc_url);
        let request = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(payload.to_vec());

        let response: StorePayloadResult = GmpApi::request_json(request).with_current_context().await?;
        Ok(response.keccak256.trim_start_matches("0x").to_string())
    }
    async fn get_payload(&self, hash: &str) -> Result<String, GmpApiError> {
        let url = format!("{}/payloads/0x{}", self.rpc_url, hash.to_lowercase());
        let request = self.client.get(&url);
        let response = GmpApi::request_bytes_if_success(request).await?;
        Ok(hex::encode(response))
    }

    fn map_cannot_execute_message_to_event(
        &self,
        id: String,
        message_id: String,
        source_chain: String,
        details: String,
        reason: CannotExecuteMessageReason,
    ) -> Event {
        Event::CannotExecuteMessageV2 {
            common: CommonEventFields {
                r#type: "CANNOT_EXECUTE_MESSAGE/V2".to_owned(),
                event_id: format!("cannot-execute-task-v{}-{}", 2, id),
                meta: None,
            },
            message_id,
            source_chain,
            reason,
            details,
        }
    }

    async fn cannot_execute_message(
        &self,
        id: String,
        message_id: String,
        source_chain: String,
        details: String,
        reason: CannotExecuteMessageReason,
    ) -> Result<(), GmpApiError> {
        let cannot_execute_message_event =
            self.map_cannot_execute_message_to_event(id, message_id, source_chain, details, reason);

        self.post_events(vec![cannot_execute_message_event]).await?;

        Ok(())
    }

    async fn its_interchain_transfer(&self, xrpl_message: XRPLMessage) -> Result<(), GmpApiError> {
        let event = match xrpl_message {
            XRPLMessage::InterchainTransferMessage(message) => Event::ITSInterchainTransfer {
                common: CommonEventFields {
                    r#type: "ITS/INTERCHAIN_TRANSFER".to_owned(),
                    event_id: format!("its-interchain-transfer-{}", message.tx_id),
                    meta: None,
                },
                message_id: message.tx_id.to_string(),
                destination_chain: message.destination_chain.to_string(),
                token_spent: Amount {
                    token_id: None,
                    amount: message.transfer_amount.to_string(),
                },
                source_address: message.source_address.to_string(),
                destination_address: message.destination_address.to_string(),
                data_hash: "0".repeat(32),
            },
            _ => {
                return Err(GmpApiError::GenericError(format!(
                    "Cannot send ITSInterchainTransfer event for message: {:?}",
                    xrpl_message
                )));
            }
        };

        self.post_events(vec![event]).await?;

        Ok(())
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait GmpApiTrait {
    fn get_chain(&self) -> &str;
    fn get_tasks_action(
        &self,
        after: Option<String>,
    ) -> impl Future<Output = Result<Vec<Task>, GmpApiError>>;
    fn post_events(
        &self,
        events: Vec<Event>,
    ) -> impl Future<Output = Result<Vec<PostEventResult>, GmpApiError>>;
    fn post_broadcast(
        &self,
        contract_address: String,
        data: &BroadcastRequest,
    ) -> impl Future<Output = Result<String, GmpApiError>>;
    fn get_broadcast_result(
        &self,
        contract_address: String,
        broadcast_id: String,
    ) -> impl Future<Output = Result<String, GmpApiError>>;
    fn post_query(
        &self,
        contract_address: String,
        data: &QueryRequest,
    ) -> impl Future<Output = Result<String, GmpApiError>>;
    fn post_payload(&self, payload: &[u8]) -> impl Future<Output = Result<String, GmpApiError>>;
    fn get_payload(&self, hash: &str) -> impl Future<Output = Result<String, GmpApiError>>;
    fn cannot_execute_message(
        &self,
        id: String,
        message_id: String,
        source_chain: String,
        details: String,
        reason: CannotExecuteMessageReason,
    ) -> impl Future<Output = Result<(), GmpApiError>>;
    fn its_interchain_transfer(
        &self,
        xrpl_message: XRPLMessage,
    ) -> impl Future<Output = Result<(), GmpApiError>>;

    fn map_cannot_execute_message_to_event(
        &self,
        id: String,
        message_id: String,
        source_chain: String,
        details: String,
        reason: CannotExecuteMessageReason,
    ) -> Event;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures;
    use httpmock::prelude::*;
    use httpmock::Method::{GET, POST};
    use reqwest::Client;
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    fn mock_gmp_api_client(mock_url: &str, chain: &str) -> GmpApi {
        let client = ClientBuilder::new(Client::new()).build();

        GmpApi {
            rpc_url: mock_url.to_string(),
            client,
            chain: chain.to_string(),
        }
    }

    fn read_json_file(file_path: &str) -> serde_json::Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file_path);
        let json_content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        serde_json::from_str(&json_content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e))
    }

    #[tokio::test]
    async fn test_get_tasks_action_success() {
        let server = MockServer::start();
        let chain = "testchain";

        let execute_tasks = read_json_file("testdata/gmp_tasks/valid_tasks/ExecuteTask.json");
        let gateway_tx_tasks = read_json_file("testdata/gmp_tasks/valid_tasks/GatewayTxTask.json");

        let tasks = vec![execute_tasks[0].clone(), gateway_tx_tasks[0].clone()];

        let mock = server.mock(|when, then| {
            when.method(GET).path(format!("/chains/{}/tasks", chain));

            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "tasks": tasks
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), chain);
        let result = gmp_api.get_tasks_action(None).await;

        mock.assert();

        assert!(result.is_ok(), "Expected success, got: {:?}", result);
        let tasks = result.unwrap();
        assert_eq!(tasks.len(), 2);

        match &tasks[0] {
            Task::Execute(task) => {
                assert_eq!(task.common.id, "execute_task_123");
                assert_eq!(task.common.chain, "xrpl");
                assert_eq!(task.common.r#type, "EXECUTE");
                assert_eq!(task.task.message.message_id, "msg_456");
                assert_eq!(task.task.message.source_chain, "polygon");
                assert_eq!(task.task.payload, "execute_payload");
            }
            _ => panic!("Expected Execute task, got: {:?}", tasks[0]),
        }

        match &tasks[1] {
            Task::GatewayTx(task) => {
                assert_eq!(task.common.id, "gateway_tx_123");
                assert_eq!(task.common.chain, "xrpl");
                assert_eq!(task.common.r#type, "GATEWAY_TX");
                assert_eq!(task.task.execute_data, "base64_encoded_data");
            }
            _ => panic!("Expected GatewayTx task, got: {:?}", tasks[1]),
        }
    }

    #[tokio::test]
    async fn test_get_tasks_action_with_after_param() {
        let server = MockServer::start();
        let chain = "testchain";
        let after = "last_task_id";

        let construct_proof_tasks =
            read_json_file("testdata/gmp_tasks/valid_tasks/ConstructProofTask.json");

        let mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/chains/{}/tasks", chain))
                .query_param("after", after);

            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "tasks": [construct_proof_tasks[0].clone()]
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), chain);
        let result = gmp_api.get_tasks_action(Some(after.to_string())).await;

        mock.assert();

        assert!(result.is_ok(), "Expected success, got: {:?}", result);
        let tasks = result.unwrap();
        assert_eq!(tasks.len(), 1);

        match &tasks[0] {
            Task::ConstructProof(task) => {
                assert_eq!(task.common.r#type, "CONSTRUCT_PROOF");
            }
            _ => panic!("Expected ConstructProof task, got: {:?}", tasks[0]),
        }
    }

    #[tokio::test]
    async fn test_get_tasks_action_error_response() {
        let server = MockServer::start();
        let chain = "testchain";

        let mock = server.mock(|when, then| {
            when.method(GET).path(format!("/chains/{}/tasks", chain));

            then.status(400)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "error": "Bad request"
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), chain);
        let result = gmp_api.get_tasks_action(None).await;

        mock.assert();

        assert!(result.is_err(), "Expected error, got: {:?}", result);
        match result {
            Err(GmpApiError::ErrorResponse(msg)) => {
                assert!(
                    msg.contains("400 Bad Request"),
                    "Unexpected error message: {}",
                    msg
                );
            }
            _ => panic!("Expected ErrorResponse, got: {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_get_tasks_action_invalid_response() {
        let server = MockServer::start();
        let chain = "testchain";

        let mock = server.mock(|when, then| {
            when.method(GET).path(format!("/chains/{}/tasks", chain));

            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "not_tasks": []
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), chain);
        let result = gmp_api.get_tasks_action(None).await;

        mock.assert();

        assert!(result.is_err(), "Expected error, got: {:?}", result);
        match result {
            Err(GmpApiError::InvalidResponse(msg)) => {
                assert_eq!(msg, "Missing 'tasks' field");
            }
            _ => panic!("Expected InvalidResponse, got: {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_post_events_success() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/chains/testchain/events")
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "events": [fixtures::gas_refunded_event()]
                }));

            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "results": [
                        {
                            "status": "success",
                            "index": 0,
                            "error": null,
                            "retriable": null
                        }
                    ]
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), "testchain");

        let events = vec![fixtures::gas_refunded_event()];
        let result = gmp_api.post_events(events).await;

        mock.assert();

        assert!(result.is_ok(), "Expected success, got: {:?}", result);
        let post_results = result.unwrap();
        assert_eq!(post_results.len(), 1);
        assert_eq!(post_results[0].status, "success");
        assert_eq!(post_results[0].index, 0);
        assert!(post_results[0].error.is_none());
        assert!(post_results[0].retriable.is_none());
    }

    #[tokio::test]
    async fn test_post_events_error_response() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/chains/testchain/events")
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "events": [fixtures::gas_refunded_event()]
                }));

            then.status(400)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "error": "Bad request"
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), "testchain");

        let events = vec![fixtures::gas_refunded_event()];
        let result = gmp_api.post_events(events).await;

        mock.assert();

        assert!(result.is_err(), "Expected error, got: {:?}", result);
        match result {
            Err(GmpApiError::ErrorResponse(msg)) => {
                assert!(
                    msg.contains("400 Bad Request"),
                    "Unexpected error message: {}",
                    msg
                );
            }
            _ => panic!("Expected ErrorResponse, got: {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_post_events_multiple() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/chains/testchain/events")
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "events": [
                        fixtures::gas_refunded_event(),
                        fixtures::gas_credit_event(),
                        fixtures::message_executed_event()
                    ]
                }));

            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "results": [
                        {
                            "status": "success",
                            "index": 0,
                            "error": null,
                            "retriable": null
                        },
                        {
                            "status": "success",
                            "index": 1,
                            "error": null,
                            "retriable": null
                        },
                        {
                            "status": "success",
                            "index": 2,
                            "error": null,
                            "retriable": null
                        }
                    ]
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), "testchain");

        let events = vec![
            fixtures::gas_refunded_event(),
            fixtures::gas_credit_event(),
            fixtures::message_executed_event(),
        ];
        let result = gmp_api.post_events(events).await;

        mock.assert();

        assert!(result.is_ok(), "Expected success, got: {:?}", result);
        let post_results = result.unwrap();
        assert_eq!(post_results.len(), 3);
        for (i, result) in post_results.iter().enumerate() {
            assert_eq!(result.status, "success");
            assert_eq!(result.index, i);
            assert!(result.error.is_none());
            assert!(result.retriable.is_none());
        }
    }

    #[tokio::test]
    async fn test_post_events_partial_success() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/chains/testchain/events")
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "events": [
                        fixtures::gas_refunded_event(),
                        fixtures::gas_credit_event()
                    ]
                }));

            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(json!({
                    "results": [
                        {
                            "status": "success",
                            "index": 0,
                            "error": null,
                            "retriable": null
                        },
                        {
                            "status": "error",
                            "index": 1,
                            "error": "Invalid event",
                            "retriable": true
                        }
                    ]
                }));
        });

        let gmp_api = mock_gmp_api_client(&server.base_url(), "testchain");

        let events = vec![fixtures::gas_refunded_event(), fixtures::gas_credit_event()];
        let result = gmp_api.post_events(events).await;

        mock.assert();

        assert!(result.is_ok(), "Expected success, got: {:?}", result);
        let post_results = result.unwrap();
        assert_eq!(post_results.len(), 2);

        assert_eq!(post_results[0].status, "success");
        assert_eq!(post_results[0].index, 0);
        assert!(post_results[0].error.is_none());
        assert!(post_results[0].retriable.is_none());

        assert_eq!(post_results[1].status, "error");
        assert_eq!(post_results[1].index, 1);
        assert_eq!(post_results[1].error, Some("Invalid event".to_string()));
        assert_eq!(post_results[1].retriable, Some(true));
    }
}
