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
    pub funder: String
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub mainnet: NetworkConfig,
    pub testnet: NetworkConfig,
    pub devnet: NetworkConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AxelarContracts {
    pub xrpl_gateway: String,
    pub xrpl_multisig_prover: String,
    pub xrpl_voting_verifier: String,
    pub multisig: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NetworkConfig {
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
}

impl Config {
    pub fn get_network(self, network: &str) -> NetworkConfig {
        match network {
            "mainnet" => self.mainnet,
            "testnet" => self.testnet,
            "devnet" => self.devnet,
            _ => panic!("Invalid network: {}", network),
        }
    }
}

impl NetworkConfig {
    pub fn from_yaml(path: &str, network: &str) -> Result<Self> {
        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let config_path = project_root.join(path);

        let content = fs::read_to_string(config_path.clone())
            .with_context(|| format!("Failed to read config file: {:?}", config_path))?;

        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config from {:?}", config_path))?;

        Ok(config.get_network(network))
    }
}
