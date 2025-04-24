use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::HashMap, env, fs, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HeartbeatsConfig {
    pub subscriber: String,
    pub distributor: String,
    pub includer: String,
    pub ingestor: String,
    pub ticket_creator: String,
    pub funder: String,
    pub price_feed: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AxelarContracts {
    pub xrpl_gateway: String,
    pub xrpl_multisig_prover: String,
    pub xrpl_voting_verifier: String,
    pub multisig: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PriceFeedConfig {
    pub pairs: Vec<String>,
    pub coin_ids: HashMap<String, HashMap<String, String>>,
    pub auth: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    pub refund_manager_addresses: String,
    pub includer_secrets: String,
    pub queue_address: String,
    pub gmp_api_url: String,
    pub xrpl_rpc: String,
    pub xrpl_faucet_url: String,
    pub xrpl_multisig: String,
    pub axelar_contracts: AxelarContracts,
    pub token_conversion_rates: HashMap<String, f64>,
    pub redis_server: String,
    pub xrpl_relayer_sentry_dsn: String,
    pub chain_name: String,
    pub client_cert_path: String,
    pub client_key_path: String,
    pub heartbeats: HeartbeatsConfig,
    pub price_feed: PriceFeedConfig,
}

impl Config {
    pub fn from_yaml(path: &str) -> Result<Self> {
        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let config_path = project_root.join(path);

        let content = fs::read_to_string(config_path.clone())
            .with_context(|| format!("Failed to read config file: {:?}", config_path))?;

        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config from {:?}", config_path))
    }
}
