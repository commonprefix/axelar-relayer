use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signature::Signature};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolanaTransaction {
    pub signature: Signature,
    pub transaction: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub logs: Vec<String>,
    pub slot: u64,
    pub cost_in_lamports: u64,
}
