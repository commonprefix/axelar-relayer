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
    // Do not send less than this (even if the cost is smaller, because we'll be refunded)
    pub execute_send_min: u64,
    pub execute_base: u64,
    pub execute_payload: u64,
    pub execute_storage_slippage: u64,
    // Approve will always be refunded to us, and theoretical maximum is 0.5 ton
    pub approve_send: u64,
    pub highload_wallet_send: u64,
    // Safe minimum to execute ITS. We will refund if it's less than this
    pub its_execute_minimum: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TONConfig {
    #[serde(flatten)]
    pub common_config: Config,

    pub wallets: Vec<WalletConfig>,
    pub ton_gateway: String,
    pub ton_gas_service: String,
    pub ton_its: String,
    pub ton_rpc: String,
    pub ton_api_key: String,
    pub gas_estimates: GasEstimates,
}
