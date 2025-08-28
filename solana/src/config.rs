use relayer_base::config::Config;
use serde_derive::Deserialize;
use solana_sdk::commitment_config::CommitmentConfig;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SolanaConfig {
    #[serde(flatten)]
    pub common_config: Config,

    pub refund_manager_addresses: String,
    pub includer_secrets: String,
    pub solana_poll_rpc: String,
    pub solana_stream_rpc: String,
    pub solana_faucet_url: String,
    pub solana_gas_service: String,
    pub solana_gateway: String,
    pub solana_commitment: CommitmentConfig,
}
