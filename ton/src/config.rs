use relayer_base::config::Config;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WalletConfig {
    pub public_key: String,
    pub secret_key: String,
    pub subwallet_id: u32,
    pub timeout: u64,
    pub address: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TONConfig {
    #[serde(flatten)]
    pub common_config: Config,

    pub wallets: Vec<WalletConfig>,
    pub ton_gateway: String,
    pub ton_gas_service: String,
    pub ton_rpc: String,
    pub ton_api_key: String,
}
