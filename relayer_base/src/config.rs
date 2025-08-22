/*!

Configuration parser.

# Example Usage

```rust,no_run
// In your chain module, create a config.rs with your configuration that includes common configuration parameters:
use serde::Deserialize;
use relayer_base::config::{config_from_yaml, Config};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MyConfig {
    #[serde(flatten)]
    pub common_config: Config,

    pub your_chain_specific_setting_1: String,
    pub your_chain_specific_setting_2: String,
    pub your_chain_specific_setting_3: String,
}

// When you need to load the configuration, use config_from_yaml:
let network = std::env::var("NETWORK").expect("NETWORK must be set");
let config: MyConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();
```

# Notes

common_config (Config) should be used for configuration options that are common to all chains and
should be safe to pass to any relayer_base function.

# TODO

- Move reading network environment variable directly to config_from_yaml.
*/

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::HashMap, env, fs, path::PathBuf};
use tracing::info;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AxelarContracts {
    pub chain_gateway: String,
    pub chain_multisig_prover: String,
    pub chain_voting_verifier: String,
    pub multisig: String, // This parameter can probably go to common Config
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PriceFeedConfig {
    pub pairs: Vec<String>,
    pub coin_ids: HashMap<String, HashMap<String, String>>,
    pub auth: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenConfig {
    pub id: String,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    pub queue_address: String,
    pub gmp_api_url: String,
    pub deployed_tokens: Vec<TokenConfig>,
    pub redis_server: String,
    pub postgres_url: String,
    pub chain_name: String,
    pub client_cert_path: String,
    pub client_key_path: String,
    pub heartbeats: HashMap<String, String>,
    pub price_feed: PriceFeedConfig,
    pub refunds_enabled: bool,
    pub demo_tokens_rate: HashMap<String, f64>,
    pub sentry_dsn: String,
    pub axelar_contracts: AxelarContracts,
    pub num_workers: u16,
}

impl Config {
    pub fn get_token_config(&self, token_id: &str) -> Option<&TokenConfig> {
        self.deployed_tokens
            .iter()
            .find(|token| token.id == token_id)
    }

    pub fn get_token_symbol(&self, token_id: &str) -> Option<&str> {
        self.get_token_config(token_id)
            .map(|token| token.symbol.as_str())
    }

    pub fn get_token_by_symbol(&self, symbol: &str) -> Option<&TokenConfig> {
        self.deployed_tokens
            .iter()
            .find(|token| token.symbol == symbol)
    }

    pub fn get_token_decimals(&self, token_id: &str) -> Option<u8> {
        self.get_token_config(token_id).map(|token| token.decimals)
    }
}

pub fn config_from_yaml<T>(path: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let base_path = env::var("BASE_PATH").ok();

    let project_root = if let Some(path) = base_path {
        PathBuf::from(path)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    };

    let config_path = project_root.join("./config/").join(path);

    info!("Loading configuration from {:?}", config_path);

    let content = fs::read_to_string(config_path.clone())
        .with_context(|| format!("Failed to read config file: {:?}", config_path))?;

    let parsed = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML config from {:?}", config_path))?;

    Ok(parsed)
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    #[derive(Debug, Clone, Deserialize, Default)]
    struct ChainConfig {
        #[serde(flatten)]
        pub common_config: Config,

        pub chain_foo: String,
        pub chain_bar: String,
        pub chain_baz: bool,
    }

    #[test]
    fn test_from_yaml() {
        let config_dir = Path::new("/tmp/config");
        fs::create_dir_all(config_dir).expect("Failed to create /tmp/config");

        let file_path = config_dir.join("test_config.yaml");

        let yaml_content = r#"
refund_manager_addresses: "manager"
chain_foo: "foo"
queue_address: "queue"
gmp_api_url: "http://api.url"
chain_bar: "bar"
chain_baz: true
redis_server: "redis"
postgres_url: "postgres"
sentry_dsn: "dsn"
chain_name: "mainnet"
client_cert_path: "/cert.pem"
client_key_path: "/key.pem"
refunds_enabled: true

axelar_contracts:
  chain_gateway: "gateway"
  chain_multisig_prover: "prover"
  chain_voting_verifier: "verifier"
  multisig: "multisig"

deployed_tokens: []
demo_tokens_rate: {}
heartbeats: {}
price_feed:
  pairs: []
  coin_ids: {}
  auth: {}

num_workers: 5
"#;

        let mut file = File::create(&file_path).expect("Failed to create YAML config file");
        file.write_all(yaml_content.as_bytes())
            .expect("Failed to write config");

        env::set_var("BASE_PATH", "/tmp");

        let config: ChainConfig =
            config_from_yaml("test_config.yaml").expect("Failed to load config");
        fs::remove_file(file_path).ok();

        assert_eq!(config.chain_foo, "foo");
        assert_eq!(config.chain_bar, "bar");
        assert!(config.chain_baz);
        assert_eq!(config.common_config.chain_name, "mainnet");
        assert!(config.common_config.price_feed.pairs.is_empty());
    }
}
