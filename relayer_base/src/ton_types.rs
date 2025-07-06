// This is probably not the right place for ton types. However, these types are our own construct
// and they don't make a lot of sense to belong in a separate, reusable project. 

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionsResponse {
    pub transactions: Vec<Transaction>,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transaction {
    pub account: String,
    pub hash: String,
    #[serde_as(as = "DisplayFromStr")]
    pub lt: i64,
    pub now: u64,
    pub mc_block_seqno: u64,
    pub trace_id: String,
    pub prev_trans_hash: String,
    pub prev_trans_lt: String,
    pub orig_status: String,
    pub end_status: String,
    pub total_fees: String,
    pub total_fees_extra_currencies: ExtraCurrencies,
    pub description: TransactionDescription,
    pub block_ref: BlockRef,
    pub in_msg: Option<TransactionMessage>,
    pub out_msgs: Vec<TransactionMessage>,
    pub account_state_before: AccountState,
    pub account_state_after: AccountState,
    pub emulated: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtraCurrencies {
    #[serde(flatten)]
    pub map: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionDescription {
    #[serde(rename = "type")]
    pub tx_type: String,
    pub aborted: bool,
    pub destroyed: bool,
    pub credit_first: bool,
    pub storage_ph: StoragePhase,
    pub credit_ph: CreditPhase,
    pub compute_ph: ComputePhase,
    #[serde(default)]
    pub action: Option<Action>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoragePhase {
    pub storage_fees_collected: String,
    pub status_change: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreditPhase {
    pub credit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComputePhase {
    pub skipped: bool,
    pub success: bool,
    pub msg_state_used: bool,
    pub account_activated: bool,
    pub gas_fees: String,
    pub gas_used: String,
    pub gas_limit: String,
    pub mode: u32,
    pub exit_code: i32,
    pub vm_steps: u64,
    pub vm_init_state_hash: String,
    pub vm_final_state_hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Action {
    pub success: bool,
    pub valid: bool,
    pub no_funds: bool,
    pub status_change: String,
    #[serde(default)]
    pub total_fwd_fees: Option<String>,
    #[serde(default)]
    pub total_action_fees: Option<String>,
    pub result_code: i32,
    pub tot_actions: u32,
    pub spec_actions: u32,
    pub skipped_actions: u32,
    pub msgs_created: u32,
    pub action_list_hash: String,
    pub tot_msg_size: MessageSize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageSize {
    pub cells: String,
    pub bits: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlockRef {
    pub workchain: i32,
    pub shard: String,
    pub seqno: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionMessage {
    pub hash: String,
    pub source: String,
    pub destination: Option<String>,
    pub value: Option<String>,
    pub value_extra_currencies: Option<ExtraCurrencies>,
    pub fwd_fee: Option<String>,
    pub ihr_fee: Option<String>,
    pub created_lt: String,
    pub created_at: String,
    pub opcode: Option<String>,
    pub ihr_disabled: Option<bool>,
    pub bounce: Option<bool>,
    pub bounced: Option<bool>,
    pub import_fee: Option<String>,
    pub message_content: MessageContent,
    pub init_state: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageContent {
    pub hash: String,
    pub body: String,
    pub decoded: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountState {
    pub hash: String,
    pub balance: Option<String>,
    pub extra_currencies: Option<ExtraCurrencies>,
    pub account_status: Option<String>,
    pub frozen_hash: Option<String>,
    pub data_hash: Option<String>,
    pub code_hash: Option<String>,
}

