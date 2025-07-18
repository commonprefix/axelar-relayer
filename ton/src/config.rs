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
pub struct GasEstimates {
    pub native_gas_refund: u64,
    pub native_gas_refund_storage_slippage: u64,
    pub execute: u64,
    pub execute_storage_slippage: u64,
    pub approve_fixed: u64,
    pub approve_fixed_storage_slippage: u64,
    pub approve_per_message: u64,
    pub approve_per_message_storage_slippage: u64,
    pub highload_wallet_per_action: u64

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
    pub gas_estimates: GasEstimates
}
