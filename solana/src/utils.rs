use chrono::Utc;
use serde_json::json;
use std::str::FromStr;
use tracing::error;

use solana_rpc_client_api::response::{
    Response, RpcConfirmedTransactionStatusWithSignature, RpcLogsResponse,
};
use solana_sdk::{
    commitment_config::{CommitmentConfig, CommitmentLevel},
    signature::Signature,
};
use solana_types::solana_types::SolanaTransaction;

pub fn create_solana_tx_from_rpc_response(
    response: Response<RpcLogsResponse>,
) -> Result<SolanaTransaction, anyhow::Error> {
    if response.value.err.is_some() {
        return Err(anyhow::anyhow!(
            "Error in transaction: {:?}",
            response.value.err
        ));
    }
    let solana_tx = SolanaTransaction {
        signature: Signature::from_str(&response.value.signature)?,
        slot: response.context.slot as i64,
        timestamp: Some(Utc::now()),
        logs: response.value.logs,
    };

    Ok(solana_tx)
}

pub fn get_tx_batch_command(
    txs: Vec<RpcConfirmedTransactionStatusWithSignature>,
    commitment: CommitmentConfig,
) -> String {
    let cfg = json!({
        "commitment": get_commitment_str(commitment),
        "maxSupportedTransactionVersion": 0,
        "encoding": "json",
    });

    let mut batch = Vec::with_capacity(txs.len());

    for (i, status_with_signature) in txs.into_iter().enumerate() {
        let sig_str = status_with_signature.signature;

        if let Err(e) = Signature::from_str(&sig_str) {
            error!("Error parsing signature: {}", e);
            continue;
        }

        batch.push(json!({
            "jsonrpc": "2.0",
            "id": (i + 1) as u64,
            "method": "getTransaction",
            "params": [ sig_str, cfg ],
        }));
    }

    serde_json::to_string(&batch).unwrap_or_else(|_| "[]".to_string())
}

pub async fn exec_curl_batch(url: &str, body_json: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("bash")
        .arg("-lc")
        .arg(format!(
            "curl '{}' -s -X POST -H 'Content-Type: application/json' --data-binary \"$BODY\"",
            url
        ))
        .env("BODY", body_json)
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Command failed with status: {}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

fn get_commitment_str(commitment: CommitmentConfig) -> String {
    match commitment.commitment {
        CommitmentLevel::Processed => String::from("processed"),
        CommitmentLevel::Confirmed => String::from("confirmed"),
        CommitmentLevel::Finalized => String::from("finalized"),
    }
}
