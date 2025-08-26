use relayer_base::config::Config;
use serde_derive::Deserialize;
use solana_sdk::commitment_config::CommitmentConfig;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SolanaConfig {
    #[serde(flatten)]
    pub common_config: Config,

    pub refund_manager_addresses: String,
    pub includer_secrets: String,
    pub solana_rpc: String,
    pub solana_faucet_url: String,
    pub solana_multisig: String,
    pub solana_gateway: String,
    pub solana_commitment: CommitmentConfig,
}
