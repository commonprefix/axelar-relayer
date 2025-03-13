use std::env;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub refund_manager_addresses: String,
    pub includer_secrets: String,
    pub instance_id: String,
    pub queue_address: String,
    pub gmp_api_url: String,
    pub payload_cache: String,
    pub xrpl_rpc: String,
    pub xrpl_multisig: String,
    pub xrpl_gateway_address: String,
    pub xrpl_multisig_prover_address: String,
    pub redis_server: String,
    pub payload_cache_auth_token: String,
    pub xrpl_relayer_sentry_dsn: String,
    pub chain_name: String,
    pub client_cert_path: String,
    pub client_key_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            refund_manager_addresses: env::var("REFUND_MANAGER_ADDRESSES")
                .context("Missing REFUND_MANAGER_ADDRESSES")?,
            includer_secrets: env::var("INCLUDER_SECRETS").context("Missing INCLUDER_SECRETS")?,
            instance_id: env::var("INSTANCE_ID").context("Missing INSTANCE_ID")?,
            queue_address: env::var("QUEUE_ADDRESS").context("Missing QUEUE_ADDRESS")?,
            gmp_api_url: env::var("GMP_API").context("Missing GMP_API")?,
            payload_cache: env::var("PAYLOAD_CACHE").context("Missing PAYLOAD_CACHE")?,
            xrpl_rpc: env::var("XRPL_RPC").context("Missing XRPL_RPC")?,
            xrpl_multisig: env::var("XRPL_MULTISIG").context("Missing XRPL_MULTISIG")?,
            xrpl_gateway_address: env::var("XRPL_GATEWAY_ADDRESS")
                .context("Missing XRPL_GATEWAY_ADDRESS")?,
            xrpl_multisig_prover_address: env::var("XRPL_MULTISIG_PROVER_ADDRESS")
                .context("Missing XRPL_MULTISIG_PROVER_ADDRESS")?,
            redis_server: env::var("REDIS_SERVER").context("Missing REDIS_SERVER")?,
            payload_cache_auth_token: env::var("PAYLOAD_CACHE_AUTH_TOKEN")
                .context("Missing PAYLOAD_CACHE_AUTH_TOKEN")?,
            xrpl_relayer_sentry_dsn: env::var("XRPL_RELAYER_SENTRY_DSN")
                .context("Missing XRPL_RELAYER_SENTRY_DSN")?,
            chain_name: env::var("CHAIN_NAME").context("Missing CHAIN_NAME")?,
            client_cert_path: env::var("CLIENT_CERT_PATH").context("Missing CLIENT_CERT_PATH")?,
            client_key_path: env::var("CLIENT_KEY_PATH").context("Missing CLIENT_KEY_PATH")?,
        })
    }
}
