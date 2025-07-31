/*!

TON RPC Client.

# Example Usage

```rust,no_run
#[tokio::main]
async fn main() {
    use ton::client::{RestClient, TONRpcClient};
    let client = TONRpcClient::new("https://testnet.toncenter.com".to_string(), "test".to_string(), 5, 5, 30).await.unwrap();
    let response = client.post_v3_message("test".to_string()).await.unwrap();
}
```

# Notes

Tonlib offers a client. However, in practice, it does not reliably work, has no support for v3 API,
and it is less flexible and more time consuming to get it running than writing one from scratch.

*/

use async_trait::async_trait;
use relayer_base::error::ClientError;
use relayer_base::error::ClientError::{BadRequest, BadResponse, ConnectionFailed};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
pub(crate) use ton_types::ton_types::{
    AccountState, AccountStatesResponse, Trace, TracesResponse, TracesResponseRest,
};
use tonlib_core::TonAddress;
use tracing::{error, info};

#[derive(Clone)]
#[derive(Debug)]
pub struct TONRpcClient {
    url: String,
    client: ClientWithMiddleware,
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

#[cfg_attr(any(test), mockall::automock)]
#[async_trait]
pub trait RestClient: Send + Sync {
    async fn post_v3_message(&self, boc: String) -> Result<V3MessageResponse, ClientError>;
    async fn get_traces_for_account(
        &self,
        account: Option<TonAddress>,
        trace_ids: Option<Vec<String>>,
        start_lt: Option<i64>,
    ) -> Result<Vec<Trace>, ClientError>;
    async fn get_account_states(
        &self,
        addresses: Vec<TonAddress>,
    ) -> Result<Vec<AccountState>, ClientError>;
}

impl TONRpcClient {
    pub async fn new(
        url: String,
        api_key: String,
        max_retries: u32,
        connect_timeout: u64,
        timeout: u64,
    ) -> Result<Self, ClientError> {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(max_retries);

        let client_builder = reqwest::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(connect_timeout))
            .timeout(Duration::from_secs(timeout))
            .pool_idle_timeout(Some(Duration::from_secs(300)));

        let client = ClientBuilder::new(
            client_builder
                .build()
                .map_err(|e| ConnectionFailed(e.to_string()))?,
        )
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build();

        Ok(Self {
            url,
            client,
            api_key,
        })
    }

    fn handle_non_success_response(
        &self,
        status: reqwest::StatusCode,
        text: &str,
        context: &str,
    ) -> ClientError {
        error!(
            "TON RPC request failed: {}: {}, (sent {})",
            status, text, context
        );
        if status.as_u16() == 400 {
            match serde_json::from_str::<V3ErrorResponse>(text) {
                Ok(err_body) => BadRequest(err_body.error),
                Err(err) => BadResponse(format!("Invalid 400 body: {err}")),
            }
        } else {
            BadResponse(format!("Unexpected status {status}: {text}"))
        }
    }
}

pub(crate) fn clean_json_string_full(input: &[u8]) -> String {
    let json_str = String::from_utf8_lossy(input);
    json_str
        .replace("\\u0000", "") // remove escaped nulls
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t') // keep readable controls
        .collect()
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
        } else {
            Err(self.handle_non_success_response(status, &text, boc.as_str()))
        }
    }

    async fn get_traces_for_account(
        &self,
        account: Option<TonAddress>,
        trace_ids: Option<Vec<String>>,
        start_lt: Option<i64>,
    ) -> Result<Vec<Trace>, ClientError> {
        let url = format!("{}/api/v3/traces", self.url.trim_end_matches('/'));

        let mut query_params = vec![("limit", "100".to_string())];

        if let Some(account) = account {
            query_params.push(("account", account.to_string()));
        }

        if let Some(trace_ids) = trace_ids {
            for t in trace_ids {
                query_params.push(("trace_id", t.to_string()));
            }
        }

        if let Some(lt_min_val) = start_lt {
            query_params.push(("start_lt", (lt_min_val + 1).to_string()));
        }

        info!("Fetching TON traces from: {:?} {:?}", url, query_params);

        let response = self
            .client
            .get(url)
            .header("X-API-Key", &self.api_key)
            .query(&query_params)
            .send()
            .await
            .map_err(|err| ConnectionFailed(err.to_string()))?;

        let status = response.status();
        let raw_bytes = response
            .bytes()
            .await
            .map_err(|err| BadResponse(err.to_string()))?;

        // We sometimes get bad UTF8 from the api, so let's make sure to clean it up
        let clean_text = clean_json_string_full(&raw_bytes);

        if status.is_success() {
            serde_json::from_str::<TracesResponseRest>(&clean_text)
                .map(TracesResponse::from)
                .map(|res| res.traces)
                .map_err(|err| BadResponse(format!("Failed to parse traces list: {err}")))
        } else {
            Err(self.handle_non_success_response(
                status,
                &clean_text,
                format!("{query_params:?}").as_str(),
            ))
        }
    }

    async fn get_account_states(
        &self,
        addresses: Vec<TonAddress>,
    ) -> Result<Vec<AccountState>, ClientError> {
        let url = format!("{}/api/v3/accountStates", self.url.trim_end_matches('/'));
        let mut query_params: Vec<(String, String)> =
            vec![("include_boc".to_string(), "false".to_string())];
        for address in addresses {
            query_params.push((
                "address".parse().unwrap(),
                address.to_string().as_str().parse().unwrap(),
            ))
        }

        let response = self
            .client
            .get(url)
            .query(&query_params)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .map_err(|err| ConnectionFailed(err.to_string()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| BadResponse(err.to_string()))?;

        if status.is_success() {
            serde_json::from_str::<AccountStatesResponse>(&text)
                .map(|res| res.accounts)
                .map_err(|err| BadResponse(format!("Failed to parse account states: {err}")))
        } else {
            Err(self.handle_non_success_response(status, &text, "get_account_states"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::HttpMockRequest;
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;
    use std::str::FromStr;

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

        let client = TONRpcClient::new(server.base_url(), "test".to_string(), 5, 5, 5)
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

        let client = TONRpcClient::new(server.base_url(), "test".to_string(), 5, 5, 5)
            .await
            .unwrap();

        let result = client.post_v3_message("bad".to_string()).await;

        match result {
            Err(BadRequest(msg)) => {
                assert_eq!(msg, "Invalid BOC format");
            }
            _ => panic!("Expected BadRequest error, got {result:?}"),
        }
    }

    #[tokio::test]
    async fn test_get_traces_with_start_lt() {
        let server = MockServer::start();

        let file_path = "tests/data/v3_traces.json";
        let body = std::fs::read_to_string(file_path).expect("Failed to read JSON test file");

        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v3/traces")
                .query_param(
                    "account",
                    "EQCqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqseb",
                )
                .query_param("start_lt", "2")
                .query_param("limit", "100");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(body.clone());
        });

        let client = TONRpcClient::new(server.base_url(), "test".to_string(), 5, 5, 5)
            .await
            .unwrap();

        let result = client
            .get_traces_for_account(
                Some(
                    TonAddress::from_str(
                        "0:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    )
                    .unwrap(),
                ),
                None,
                Some(1),
            )
            .await;

        assert!(
            result.is_ok(),
            "Expected successful result with start_lt, got: {result:?}"
        );

        let traces = result.unwrap();
        assert_eq!(traces.len(), 19);

        let txs = &traces[0].transactions;
        assert_eq!(txs.len(), 6);

        let tx0 = &txs[0];
        assert_eq!(tx0.now, 1751291309);
        assert_eq!(tx0.lt, 36300947000011i64);
        assert!(tx0.in_msg.is_some());

        // This really tests the deserializer
        assert_eq!(&txs[0].hash, "aa1");
        assert_eq!(&txs[1].hash, "aa2");
        assert_eq!(&txs[2].hash, "aa3");
        assert_eq!(&txs[3].hash, "aa4");
        assert_eq!(&txs[4].hash, "aa5");
    }

    #[tokio::test]
    async fn test_get_transactions_without_start_lt() {
        let server = MockServer::start();

        let file_path = "tests/data/v3_traces.json";
        let body = std::fs::read_to_string(file_path).expect("Failed to read JSON test file");

        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v3/traces")
                .matches(|req: &HttpMockRequest| {
                    if let Some(params) = &req.query_params {
                        let mut has_account = false;
                        let mut has_start_lt = false;
                        let mut has_offset = false;
                        let mut has_limit = false;

                        for (key, _) in params {
                            match key.as_str() {
                                "account" => has_account = true,
                                "start_lt" => has_start_lt = true,
                                "offset" => has_offset = true,
                                "limit" => has_limit = true,
                                _ => {}
                            }
                        }

                        has_account && !has_start_lt && !has_offset && has_limit
                    } else {
                        false
                    }
                });

            then.status(200)
                .header("Content-Type", "application/json")
                .body(body.clone());
        });

        let client = TONRpcClient::new(server.base_url(), "test".to_string(), 5, 5, 5)
            .await
            .unwrap();

        let result = client
            .get_traces_for_account(
                Some(
                    TonAddress::from_str(
                        "0:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    )
                    .unwrap(),
                ),
                None,
                None,
            )
            .await;

        assert!(
            result.is_ok(),
            "Expected successful result without start_lt, but got error: {:?}",
            result.unwrap_err()
        );
    }

    #[tokio::test]
    async fn test_post_v3_message_retries_on_failure() {
        let server = MockServer::start();

        let mock = server.mock(move |when, then| {
            when.method(POST)
                .path("/api/v3/message")
                .body(r#"{"boc":"retry"}"#);
            then.status(500).body("Internal Server Error");
        });

        let max_retries = 3;
        let client = TONRpcClient::new(server.base_url(), "test".to_string(), max_retries, 5, 30)
            .await
            .unwrap();

        let result = client.post_v3_message("retry".to_string()).await;

        assert!(result.is_err());
        mock.assert_hits_async(max_retries as usize + 1).await; // initial + retries
    }

    #[tokio::test]
    async fn test_post_v3_message_retries_on_timeout() {
        use std::time::Duration;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v3/message")
                .body(r#"{"boc":"timeout"}"#);
            then.status(200)
                .body(r#"{"message_hash":"dummy","message_hash_norm":"DUMMY"}"#)
                .delay(Duration::from_millis(1100));
        });
        let max_retries = 1;

        let client = TONRpcClient::new(server.base_url(), "test".to_string(), max_retries, 1, 1)
            .await
            .unwrap();

        let result = client.post_v3_message("timeout".to_string()).await;

        assert!(
            result.is_err(),
            "Expected request to timeout and retry, but got: {result:?}"
        );
        mock.assert_hits(max_retries as usize + 1);
    }

    #[tokio::test]
    async fn test_get_account_states() {
        let server = MockServer::start();

        let mock_response = json!({
            "accounts": [
                {
                    "address": "0:294D72EC421B930C60854F413478C162FDCEEC65746084EBACE25227182979A2",
                    "account_state_hash": "FeIPUrTpygkiYmgmMtcKR7waymJgT0rBFmvUZdSfK2A=",
                    "balance": "327063115",
                    "status": "active"
                }
            ]
        });

        server.mock(|when, then| {
            when.method(GET).path("/api/v3/accountStates");
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body(mock_response);
        });

        let client = TONRpcClient::new(server.base_url(), "test".to_string(), 3, 5, 10)
            .await
            .unwrap();

        let result = client
            .get_account_states(vec![
                "0:294D72EC421B930C60854F413478C162FDCEEC65746084EBACE25227182979A2"
                    .to_string()
                    .parse()
                    .unwrap(),
            ])
            .await;

        assert!(result.is_ok());
        let accounts = result.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0].address,
            TonAddress::from_str(
                "0:294D72EC421B930C60854F413478C162FDCEEC65746084EBACE25227182979A2"
            )
            .unwrap()
        );
        assert_eq!(accounts[0].balance, "327063115");
        assert_eq!(accounts[0].status, "active");
    }
}
