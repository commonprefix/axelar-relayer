use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::collections::HashMap;
use tonlib_core::TonAddress;

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionsResponse {
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TracesResponse {
    pub traces: Vec<Trace>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TracesResponseRest {
    pub traces: Vec<TraceRest>,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TraceRest {
    pub is_incomplete: bool,
    #[serde_as(as = "DisplayFromStr")]
    pub start_lt: i64,
    #[serde_as(as = "DisplayFromStr")]
    pub end_lt: i64,
    pub trace_id: String,
    pub transactions: HashMap<String, Transaction>,
    pub transactions_order: Vec<String>,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Trace {
    pub is_incomplete: bool,
    #[serde_as(as = "DisplayFromStr")]
    pub start_lt: i64,
    #[serde_as(as = "DisplayFromStr")]
    pub end_lt: i64,
    pub trace_id: String,
    pub transactions: Vec<Transaction>,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transaction {
    #[serde_as(as = "DisplayFromStr")]
    pub account: TonAddress,
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
    #[serde_as(as = "DisplayFromStr")]
    pub total_fees: u64,
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
    pub credit_ph: Option<CreditPhase>,
    pub compute_ph: Option<ComputePhase>,
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
    pub success: Option<bool>,
    pub msg_state_used: Option<bool>,
    pub account_activated: Option<bool>,
    pub gas_fees: Option<String>,
    pub gas_used: Option<String>,
    pub gas_limit: Option<String>,
    pub mode: Option<u32>,
    pub exit_code: Option<i32>,
    pub vm_steps: Option<u64>,
    pub vm_init_state_hash: Option<String>,
    pub vm_final_state_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Action {
    pub success: Option<bool>,
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


#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionMessage {
    pub hash: String,
    pub source: Option<String>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub destination: Option<TonAddress>,
    pub value: Option<String>,
    pub value_extra_currencies: Option<ExtraCurrencies>,
    pub fwd_fee: Option<String>,
    pub ihr_fee: Option<String>,
    pub created_lt: Option<String>,
    pub created_at: Option<String>,
    #[serde(deserialize_with = "deserialize_hex_u32")]
    pub opcode: Option<u32>,
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

impl From<TracesResponseRest> for TracesResponse {
    fn from(rest: TracesResponseRest) -> Self {
        let traces = rest
            .traces
            .into_iter()
            .map(|trace_rest| {
                let transactions = trace_rest
                    .transactions_order
                    .into_iter()
                    .map(|key| {
                        trace_rest
                            .transactions
                            .get(&key)
                            .cloned()
                            .unwrap_or_else(|| panic!("Transaction key '{}' not found in map", key))
                    })
                    .collect();

                Trace {
                    is_incomplete: trace_rest.is_incomplete,
                    start_lt: trace_rest.start_lt,
                    end_lt: trace_rest.end_lt,
                    trace_id: trace_rest.trace_id,
                    transactions,
                }
            })
            .collect();

        TracesResponse { traces }
    }
}

fn deserialize_hex_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde_json::Value;
    let val = Option::<Value>::deserialize(deserializer)?;

    match val {
        Some(Value::String(s)) => {
            let trimmed = s.trim_start_matches("0x");
            u32::from_str_radix(trimmed, 16)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|n| Some(n as u32))
            .ok_or_else(|| serde::de::Error::custom("Expected u64 number")),
        Some(_) => Err(serde::de::Error::custom("Expected string or number for opcode")),
        None => Ok(None),
    }
}
