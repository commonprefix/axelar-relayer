use anyhow::{anyhow, Result};
use chrono::{offset::Utc, DateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_sdk::signature::Signature;
use solana_transaction_status_client_types::{
    EncodedConfirmedTransactionWithStatusMeta, UiInnerInstructions,
};
use std::str::FromStr;
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SolanaTransaction {
    pub signature: Signature,
    pub timestamp: Option<DateTime<Utc>>,
    pub logs: Vec<String>,
    pub slot: i64,
    pub ixs: Vec<UiInnerInstructions>,
    pub cost_units: u64,
}

impl SolanaTransaction {
    pub fn from_rpc_response(
        rpc_response: RpcGetTransactionResponse,
    ) -> Result<Self, anyhow::Error> {
        let result = rpc_response
            .result
            .ok_or_else(|| anyhow!("No result found"))?;
        let meta = result
            .meta
            .as_ref()
            .ok_or_else(|| anyhow!("No meta found"))?;
        let logs = meta
            .log_messages
            .clone()
            .ok_or_else(|| anyhow!("No log messages found"))?;

        let slot = result.slot as i64;
        let signature = result
            .transaction
            .signatures
            .first()
            .and_then(|s| Signature::from_str(s).ok())
            .ok_or_else(|| anyhow!("Missing or invalid signature"))?;

        let timestamp = Some(Utc::now());
        let ixs = meta.inner_instructions.clone();
        let cost_units = meta.cost_units.unwrap_or(0);

        Ok(Self {
            signature,
            timestamp,
            logs,
            slot,
            ixs,
            cost_units,
        })
    }

    pub fn from_encoded_confirmed_transaction_with_status_meta(
        signature: Signature,
        tx: EncodedConfirmedTransactionWithStatusMeta,
    ) -> Result<Self, anyhow::Error> {
        let meta = &tx
            .transaction
            .meta
            .ok_or_else(|| anyhow!("No meta found"))?;
        Ok(Self {
            signature: signature.clone(),
            timestamp: None,
            logs: meta
                .log_messages
                .clone()
                .ok_or_else(|| anyhow!("No log messages found"))?,
            slot: tx.slot as i64,
            ixs: {
                meta.inner_instructions
                    .clone()
                    .ok_or_else(|| anyhow!("No inner instructions found"))?
                    .clone()
            },
            cost_units: meta.compute_units_consumed.clone().unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcGetTransactionResponse {
    pub jsonrpc: String,
    pub result: Option<RpcTransactionResult>,
    pub id: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcTransactionResult {
    #[serde(rename = "blockTime")]
    pub block_time: Option<i64>,
    pub meta: Option<RpcTransactionMeta>,
    pub slot: i64,
    pub transaction: RpcTransaction,
    pub version: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcTransactionMeta {
    #[serde(rename = "computeUnitsConsumed")]
    pub compute_units_consumed: Option<u64>,
    #[serde(rename = "costUnits")]
    pub cost_units: Option<u64>,
    pub err: Option<Value>,
    pub fee: u64,
    #[serde(rename = "innerInstructions")]
    pub inner_instructions: Vec<UiInnerInstructions>,
    #[serde(default, rename = "loadedAddresses")]
    pub loaded_addresses: Option<RpcLoadedAddresses>,
    #[serde(rename = "logMessages")]
    pub log_messages: Option<Vec<String>>,
    #[serde(default, rename = "postBalances")]
    pub post_balances: Vec<u64>,
    #[serde(default, rename = "postTokenBalances")]
    pub post_token_balances: Vec<Value>,
    #[serde(default, rename = "preBalances")]
    pub pre_balances: Vec<u64>,
    #[serde(default, rename = "preTokenBalances")]
    pub pre_token_balances: Vec<Value>,
    pub rewards: Option<Vec<Value>>,
    #[serde(default)]
    pub status: RpcStatus,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RpcLoadedAddresses {
    #[serde(default)]
    pub readonly: Vec<String>,
    #[serde(default)]
    pub writable: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RpcStatus {
    #[serde(default, rename = "Ok")]
    pub ok: Option<Value>,
    #[serde(default, rename = "Err")]
    pub err: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcTransaction {
    pub message: RpcMessage,
    pub signatures: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcMessage {
    #[serde(default, rename = "accountKeys")]
    pub account_keys: Vec<String>,
    #[serde(default)]
    pub header: RpcMessageHeader,
    #[serde(default)]
    pub instructions: Vec<RpcInstruction>,
    #[serde(default, rename = "recentBlockhash")]
    pub recent_blockhash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RpcMessageHeader {
    #[serde(default, rename = "numReadonlySignedAccounts")]
    pub num_readonly_signed_accounts: u8,
    #[serde(default, rename = "numReadonlyUnsignedAccounts")]
    pub num_readonly_unsigned_accounts: u8,
    #[serde(default, rename = "numRequiredSignatures")]
    pub num_required_signatures: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcInstruction {
    pub accounts: Vec<u8>,
    pub data: String,
    #[serde(rename = "programIdIndex")]
    pub program_id_index: u8,
    #[serde(rename = "stackHeight")]
    pub stack_height: Option<u64>,
}
