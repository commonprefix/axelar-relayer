use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use serde::Deserialize;
use std::error::Error;

#[derive(Deserialize)]
struct HashResponse {
    hash: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

/// A reusable API client for the payload storage service.
pub struct PayloadCacheClient {
    client: Client,
    base_url: String,
    auth_token: String,
}

impl PayloadCacheClient {
    pub fn new(base_url: &str, auth_token: &str) -> Self {
        PayloadCacheClient {
            client: Client::new(),
            base_url: base_url.to_string(),
            auth_token: auth_token.to_string(),
        }
    }

    pub async fn store_payload(&self, payload: &str) -> Result<String, Box<dyn Error>> {
        let resp = self
            .client
            .post(&self.base_url)
            .header(AUTHORIZATION, format!("Bearer {}", self.auth_token))
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(payload.to_string())
            .send()
            .await?;

        if resp.status().is_success() {
            let hash_resp: HashResponse = resp.json().await?;
            Ok(hash_resp.hash)
        } else {
            let error_resp: ErrorResponse = resp.json().await.unwrap_or(ErrorResponse {
                error: "Unknown error".into(),
            });
            Err(format!("Failed to store payload: {}", error_resp.error).into())
        }
    }

    pub async fn get_payload(&self, hash: &str) -> Result<String, Box<dyn Error>> {
        let get_url = format!("{}?hash={}", self.base_url, hash);
        let get_resp = self
            .client
            .get(&get_url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await?;

        if get_resp.status().is_success() {
            let returned_payload = get_resp.text().await?;
            Ok(returned_payload)
        } else {
            let error_resp: ErrorResponse = get_resp.json().await.unwrap_or(ErrorResponse {
                error: "Unknown error".into(),
            });
            Err(format!("Failed to retrieve payload: {}", error_resp.error).into())
        }
    }
}
