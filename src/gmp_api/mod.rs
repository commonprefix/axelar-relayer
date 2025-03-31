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

use crate::{config::NetworkConfig, error::GmpApiError, utils::parse_task};
use gmp_types::{
    BroadcastRequest, Event, PostEventResponse, PostEventResult, QueryRequest, StorePayloadResult,
    Task,
};

pub struct GmpApi {
    rpc_url: String,
    client: Client,
    chain: String,
}

fn identity_from_config(config: &NetworkConfig) -> Result<Identity, GmpApiError> {
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
    pub fn new(config: &NetworkConfig) -> Result<Self, GmpApiError> {
        let client = reqwest::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Some(Duration::from_secs(300)))
            .identity(identity_from_config(config)?)
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
            return GmpApiError::RequestFailed(e.to_string());
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

        let payload = match data {
            BroadcastRequest::Generic(value) => value,
        };

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
        response
    }

    pub async fn post_query(
        &self,
        contract_address: String,
        data: &QueryRequest,
    ) -> Result<String, GmpApiError> {
        let url = format!("{}/contracts/{}/queries", self.rpc_url, contract_address);

        let payload = match data {
            QueryRequest::Generic(value) => value,
        };

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
}

#[cfg(test)]
mod tests {
    use crate::gmp_types::{
        Amount, CallEvent, CommonEventFields, CommonTaskFields, ExecuteTask, ExecuteTaskFields,
        Message, RefundTask, RefundTaskFields,
    };

    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_get_tasks() {
        let response = r#"
        {
            "tasks": [
                {
                    "id": "task1",
                    "timestamp": "2024-01-01T12:00:00Z",
                    "type": "EXECUTE",
                    "task": {
                        "message": {
                            "messageID": "msg1",
                            "sourceChain": "chainA",
                            "sourceAddress": "srcAddr",
                            "destinationAddress": "destAddr",
                            "payloadHash": "payloadHash123"
                        },
                        "payload": "payloadData",
                        "availableGasBalance": {
                            "tokenID": null,
                            "amount": "1000"
                        }
                    }
                },
                {
                    "id": "task2",
                    "timestamp": "2024-01-01T12:30:00Z",
                    "type": "REFUND",
                    "task": {
                        "message": {
                            "messageID": "msg2",
                            "sourceChain": "chainB",
                            "sourceAddress": "srcAddrB",
                            "destinationAddress": "destAddrB",
                            "payloadHash": "payloadHash456"
                        },
                        "refundRecipientAddress": "refundAddr",
                        "remainingGasBalance": {
                            "tokenID": "token123",
                            "amount": "2000"
                        }
                    }
                }
            ]
        }
        "#;

        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("GET", "/chains/test/tasks")
            .with_status(200)
            .with_body(response)
            .create();

        let api = GmpApi::new(&url, "test").unwrap();

        let result = api.get_tasks_action().await;

        assert!(result.is_ok());
        let tasks = result.unwrap();

        assert_eq!(
            tasks[0],
            Task::Execute(ExecuteTask {
                common: CommonTaskFields {
                    id: "task1".to_string(),
                    timestamp: "2024-01-01T12:00:00Z".to_string(),
                    r#type: "EXECUTE".to_string(),
                },
                task: ExecuteTaskFields {
                    message: Message {
                        message_id: "msg1".to_string(),
                        source_chain: "chainA".to_string(),
                        source_address: "srcAddr".to_string(),
                        destination_address: "destAddr".to_string(),
                        payload_hash: "payloadHash123".to_string()
                    },
                    payload: "payloadData".to_string(),
                    available_gas_balance: Amount {
                        token_id: None,
                        amount: "1000".to_string()
                    }
                }
            })
        );

        assert_eq!(
            tasks[1],
            Task::Refund(RefundTask {
                common: CommonTaskFields {
                    id: "task2".to_string(),
                    timestamp: "2024-01-01T12:30:00Z".to_string(),
                    r#type: "REFUND".to_string(),
                },
                task: RefundTaskFields {
                    message: Message {
                        message_id: "msg2".to_string(),
                        source_chain: "chainB".to_string(),
                        source_address: "srcAddrB".to_string(),
                        destination_address: "destAddrB".to_string(),
                        payload_hash: "payloadHash456".to_string()
                    },
                    refund_recipient_address: "refundAddr".to_string(),
                    remaining_gas_balance: Amount {
                        token_id: Some("token123".to_string()),
                        amount: "2000".to_string()
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn test_post_events_response_parsing() {
        let response = r#"
    {
        "results": [
            {
                "status": "ACCEPTED",
                "index": 0
            },
            {
                "status": "REJECTED",
                "index": 1,
                "error": "Invalid event data",
                "retriable": true
            }
        ]
    }
    "#;

        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("POST", "/chains/test/events")
            .with_status(200)
            .with_body(response)
            .create();

        let api = GmpApi::new(&url, "test").unwrap();

        let events = vec![];
        let result = api.post_events(events).await;

        match result {
            Ok(results) => {
                assert_eq!(results.len(), 2);
                assert_eq!(results[0].status, "ACCEPTED");
                assert_eq!(results[0].index, 0);
                assert_eq!(results[0].error, None);
                assert_eq!(results[0].retriable, None);

                assert_eq!(results[1].status, "REJECTED");
                assert_eq!(results[1].index, 1);
                assert_eq!(results[1].error.as_deref(), Some("Invalid event data"));
                assert_eq!(results[1].retriable, Some(true));
            }
            Err(e) => panic!("Failed to post events: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_post_events_verify_request_body() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let response_body = r#"
            {
                "results": [
                    {
                        "status": "ACCEPTED",
                        "index": 0
                    }
                ]
            }
        "#;

        let _mock = server
            .mock("POST", "/chains/test/events")
            .match_body(r#"{"events":[{"type":"CALL","eventID":"event1","message":{"messageID":"msg1","sourceChain":"chainA","sourceAddress":"srcAddrA","destinationAddress":"destAddrA","payloadHash":"payload123"},"destinationChain":"chainB","payload":"payloadData","meta":null}]}"#)
            .with_status(200)
            .with_body(response_body)
            .create_async()
            .await;

        let api = GmpApi::new(&url, "test").unwrap();

        let events = vec![Event::Call(CallEvent {
            common: CommonEventFields {
                r#type: "CALL".to_string(),
                event_id: "event1".to_string(),
            },
            message: Message {
                message_id: "msg1".to_string(),
                source_chain: "chainA".to_string(),
                source_address: "srcAddrA".to_string(),
                destination_address: "destAddrA".to_string(),
                payload_hash: "payload123".to_string(),
            },
            destination_chain: "chainB".to_string(),
            payload: "payloadData".to_string(),
            meta: None,
        })];

        let result = api.post_events(events).await;

        assert!(result.is_ok());
    }
}
