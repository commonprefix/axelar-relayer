use std::time::Duration;

use serde::{de::DeserializeOwned, Serialize};
use tracing::warn;
use xrpl_api::Request;

use crate::error::ClientError;

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(3);
pub struct XRPLClient {
    client: xrpl_http_client::Client,
    max_retries: usize,
}

impl XRPLClient {
    pub fn inner(&self) -> &xrpl_http_client::Client {
        &self.client
    }

    pub async fn call<Req>(
        &self,
        request: Req,
    ) -> Result<Req::Response, xrpl_http_client::error::Error>
    where
        Req: Request + Serialize + std::fmt::Debug + std::clone::Clone,
        Req::Response: DeserializeOwned,
    {
        let mut retries = 0;
        let mut delay = Duration::from_millis(500);

        loop {
            match self.client.call(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if retries >= self.max_retries {
                        return Err(e);
                    }

                    warn!(
                        "RPC call ({}) failed (retry {}/{}): {}. Retrying in {:?}...",
                        request.method(),
                        retries + 1,
                        self.max_retries,
                        e,
                        delay
                    );

                    tokio::time::sleep(delay).await;
                    retries += 1;
                    delay = delay.mul_f32(2.0);
                }
            }
        }
    }

    pub fn new(url: &str, max_retries: usize) -> Result<Self, ClientError> {
        let http_client = reqwest::ClientBuilder::new()
            .connect_timeout(DEFAULT_RPC_TIMEOUT)
            .timeout(DEFAULT_RPC_TIMEOUT)
            .build()
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            client: xrpl_http_client::Client::builder()
                .base_url(url)
                .http_client(http_client)
                .build(),
            max_retries,
        })
    }
}
