use serde_derive::Deserialize;
use relayer_base::config::{Config};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct XRPLConfig {

    #[serde(flatten)]
    pub common_config: Config,

    pub refund_manager_addresses: String,
    pub includer_secrets: String,
    pub xrpl_rpc: String,
    pub xrpl_faucet_url: String,
    pub xrpl_multisig: String,
}
