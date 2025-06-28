/*!

TON RPC Client.

# Example Usage

```rust,no_run
#[tokio::main]
async fn main() {
    use ton::client::{RestClient, TONRpcClient};
    let client = TONRpcClient::new("https://testnet.toncenter.com".to_string(), 1, "test".to_string()).await.unwrap();
    let response = client.post_v3_message("test".to_string()).await.unwrap();
}
```

# TODO

- Handle retries
- Check that timeouts are handled correctly

# Notes

In principle, we should be getting similar functionality from tonlib, but in practice
it's not working reliably.

*/

use async_trait::async_trait;
use relayer_base::error::ClientError;
use relayer_base::error::ClientError::{BadRequest, BadResponse, ConnectionFailed};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

pub struct TONRpcClient {
    url: String,
    client: Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct V3MessageResponse {
    pub message_hash: String,
    pub message_hash_norm: String,
}

#[derive(Debug, Deserialize)]
pub struct V3ErrorResponse {
    pub code: i32,
    pub error: String,
}

#[async_trait]
pub trait RestClient: Send + Sync {
    async fn post_v3_message(&self, boc: String) -> Result<V3MessageResponse, ClientError>;
}

impl TONRpcClient {
    pub async fn new(
        url: String,
        _max_retries: usize,
        api_key: String,
    ) -> Result<Self, ClientError> {
        let client = Client::new();

        Ok(Self {
            url,
            client,
            api_key,
        })
    }
}

#[async_trait::async_trait]
impl RestClient for TONRpcClient {
    async fn post_v3_message(&self, boc: String) -> Result<V3MessageResponse, ClientError> {
        let body = json!({
            "boc": boc,
        });

        let url = format!("{}/api/v3/message", self.url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .header("X-API-Key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| ConnectionFailed(err.to_string()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| BadResponse(err.to_string()))?;

        if status.is_success() {
            serde_json::from_str::<V3MessageResponse>(&text)
                .map_err(|err| BadResponse(format!("Failed to parse success response: {err}")))
        } else if status.as_u16() == 400 {
            match serde_json::from_str::<V3ErrorResponse>(&text) {
                Ok(err_body) => Err(BadRequest(err_body.error)),
                Err(err) => Err(BadResponse(format!("Invalid 400 body: {err}"))),
            }
        } else {
            Err(BadResponse(format!(
                "Unexpected status {}: {}",
                status, text
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::Method::POST;
    use httpmock::MockServer;

    #[tokio::test]
    async fn test_post_v3_message() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v3/message")
                .body(r#"{"boc":"test"}"#);
            then.status(200)
                .json_body(json!({"message_hash": "abc123", "message_hash_norm": "ABC123"}));
        });

        let client = TONRpcClient::new(server.base_url(), 1, "test".to_string())
            .await
            .unwrap();
        let response = client.post_v3_message("test".to_string()).await.unwrap();
        assert_eq!(response.message_hash, "abc123");
        assert_eq!(response.message_hash_norm, "ABC123");
    }

    #[tokio::test]
    async fn test_post_v3_message_bad_request() {
        let server = MockServer::start();

        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v3/message")
                .body(r#"{"boc":"bad"}"#);
            then.status(400).json_body(json!({
                "code": 400,
                "error": "Invalid BOC format"
            }));
        });

        let client = TONRpcClient::new(server.base_url(), 1, "test".to_string())
            .await
            .unwrap();

        let result = client.post_v3_message("bad".to_string()).await;

        match result {
            Err(BadRequest(msg)) => {
                assert_eq!(msg, "Invalid BOC format");
            }
            _ => panic!("Expected BadRequest error, got {:?}", result),
        }
    }
}
