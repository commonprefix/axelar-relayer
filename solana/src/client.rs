use std::{future::Future, time::Duration};

use anyhow::anyhow;
use serde::{de::DeserializeOwned, Serialize};

use relayer_base::error::ClientError;
use solana_rpc_client::RpcClient;

pub struct SolanaClient {
    client: solana_rpc_client::Client,
    max_retries: usize,
}
