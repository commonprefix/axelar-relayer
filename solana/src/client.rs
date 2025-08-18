use std::{future::Future, time::Duration};

use anyhow::anyhow;
use serde::{de::DeserializeOwned, Serialize};

use relayer_base::error::ClientError;
use solana_rpc_client::rpc_client::RpcClient;

pub trait SolanaClientTrait: Send + Sync {
    fn inner(&self) -> &RpcClient;
}

pub struct SolanaClient {
    client: RpcClient,
    max_retries: usize,
}

impl SolanaClient {
    pub fn new(url: &str, max_retries: usize) -> Result<Self, ClientError> {
        Ok(Self {
            client: RpcClient::new(url),
            max_retries,
        })
    }
}

impl SolanaClientTrait for SolanaClient {
    fn inner(&self) -> &RpcClient {
        &self.client
    }
}
