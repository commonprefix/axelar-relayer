pub mod gmp_types;

use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self},
    path::PathBuf,
    time::Duration,
};
use tracing::{debug, info, warn};

use reqwest::{Client, Identity};

use crate::{config::Config, error::GmpApiError, utils::parse_task};
use gmp_types::{
    BroadcastRequest, CannotExecuteMessageReason, CommonEventFields, Event, PostEventResponse,
    PostEventResult, QueryRequest, StorePayloadResult, Task,
};

pub struct GmpApi {
    rpc_url: String,
    client: Client,
    chain: String,
}

fn identity_from_config(config: &Config) -> Result<Identity, GmpApiError> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

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
        let mut client = reqwest::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .identity(identity_from_config(config)?);

        if connection_pooling {
            client = client.pool_idle_timeout(Some(Duration::from_secs(300)));
        } else {
            client = client.pool_max_idle_per_host(0);
        }

        let client = client
            .build()
            .map_err(|e| GmpApiError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            rpc_url: config.gmp_api_url.to_owned(),
            client,
            chain: config.chain_name.to_owned(),
        })
    }

    async fn request_bytes_if_success(
        request: reqwest::RequestBuilder,
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
        request: reqwest::RequestBuilder,
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
        request: reqwest::RequestBuilder,
    ) -> Result<T, GmpApiError> {
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

    pub async fn get_tasks_action(&self, after: Option<String>) -> Result<Vec<Task>, GmpApiError> {
        let request_url = format!("{}/chains/{}/tasks", self.rpc_url, self.chain);
        let mut request = self.client.get(&request_url);

        if let Some(after) = after {
            request = request.query(&[("after", &after)]);
            debug!("Requesting tasks after: {}", after);
        }

        let response: HashMap<String, Vec<Value>> = GmpApi::request_json(request).await?;
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

    pub async fn post_events(
        &self,
        events: Vec<Event>,
    ) -> Result<Vec<PostEventResult>, GmpApiError> {
        let mut map = HashMap::new();
        map.insert("events", events);

        debug!("Posting events: {:?}", map);

        let url = format!("{}/chains/{}/events", self.rpc_url, self.chain);
        let request = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&map).unwrap());

        let response: PostEventResponse = GmpApi::request_json(request).await?;
        info!("Response from POST: {:?}", response);
        Ok(response.results)
    }

    pub async fn post_broadcast(
        &self,
        contract_address: String,
        data: &BroadcastRequest,
    ) -> Result<String, GmpApiError> {
        let url = format!("{}/contracts/{}/broadcasts", self.rpc_url, contract_address);

        let BroadcastRequest::Generic(payload) = data;

        debug!("Broadcast:");
        debug!("URL: {}", url);
        debug!("Payload: {}", serde_json::to_string(payload).unwrap());

        let request = self
            .client
            .post(url.clone())
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(payload).unwrap());

        let response = GmpApi::request_text_if_success(request).await;
        if response.is_ok() {
            debug!("Broadcast successful: {:?}", response.as_ref().unwrap());
        }
        // TODO: parse response and check for errors
        response
    }

    pub async fn post_query(
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
            .body(serde_json::to_string(payload).unwrap());

        GmpApi::request_text_if_success(request).await
    }

    pub async fn post_payload(&self, payload: &[u8]) -> Result<String, GmpApiError> {
        let url = format!("{}/payloads", self.rpc_url);
        let request = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(payload.to_vec());

        let response: StorePayloadResult = GmpApi::request_json(request).await?;
        Ok(response.keccak256.trim_start_matches("0x").to_string())
    }

    pub async fn get_payload(&self, hash: &str) -> Result<String, GmpApiError> {
        let url = format!("{}/payloads/0x{}", self.rpc_url, hash.to_lowercase());
        let request = self.client.get(&url);
        let response = GmpApi::request_bytes_if_success(request).await?;
        Ok(hex::encode(response))
    }

    pub async fn cannot_execute_message(
        &self,
        id: String,
        message_id: String,
        source_chain: String,
        details: String,
    ) -> Result<(), GmpApiError> {
        let cannot_execute_message_event = Event::CannotExecuteMessageV2 {
            common: CommonEventFields {
                r#type: "CANNOT_EXECUTE_MESSAGE/V2".to_owned(),
                event_id: format!("cannot-execute-task-v{}-{}", 2, id),
                meta: None,
            },
            message_id,
            source_chain,
            reason: CannotExecuteMessageReason::Error, // TODO
            details,
        };

        self.post_events(vec![cannot_execute_message_event]).await?;

        Ok(())
    }
}
