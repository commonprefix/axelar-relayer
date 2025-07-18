pub mod gmp_types;

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self},
    path::PathBuf,
    time::Duration,
};
use tracing::{debug, info, warn};
use xrpl_amplifier_types::msg::XRPLMessage;

use reqwest::Identity;

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

            let response_json: serde_json::Value = serde_json::from_str(response.as_ref().unwrap())
                .map_err(|e| {
                    GmpApiError::GenericError(format!(
                        "Failed to parse GMP response {}: {}",
                        response.as_ref().unwrap(),
                        e
                    ))
                })?;

            let broadcast_id = response_json
                .get("broadcastID")
                .ok_or_else(|| {
                    GmpApiError::GenericError("Missing 'broadcastID' field".to_string())
                })?
                .as_str()
                .ok_or_else(|| {
                    GmpApiError::GenericError("'broadcastID' field is not a string".to_string())
                })?;

            let broadcast_result = self
                .get_broadcast_result(contract_address, broadcast_id.to_string())
                .await;

            if broadcast_result.is_err()
                && matches!(
                    broadcast_result.clone().unwrap_err(),
                    GmpApiError::Timeout(_)
                )
            {
                // TODO: handle timeouts
                info!("Broadcast id {} timed out.", broadcast_id);
                return Ok("timeout".to_string());
            }

            debug!(
                "Broadcast id {} result: {:?}",
                broadcast_id, broadcast_result
            );

            return broadcast_result;
        }
        response
    }

    pub async fn get_broadcast_result(
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
                                response.get("status").unwrap().to_string(),
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
        reason: CannotExecuteMessageReason,
    ) -> Result<(), GmpApiError> {
        let cannot_execute_message_event = Event::CannotExecuteMessageV2 {
            common: CommonEventFields {
                r#type: "CANNOT_EXECUTE_MESSAGE/V2".to_owned(),
                event_id: format!("cannot-execute-task-v{}-{}", 2, id),
                meta: None,
            },
            message_id,
            source_chain,
            reason,
            details,
        };

        self.post_events(vec![cannot_execute_message_event]).await?;

        Ok(())
    }

    pub async fn its_interchain_transfer(
        &self,
        xrpl_message: XRPLMessage,
    ) -> Result<(), GmpApiError> {
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
