use chrono::Utc;
use std::str::FromStr;

use solana_rpc_client_api::response::{Response, RpcLogsResponse};
use solana_sdk::signature::Signature;
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
        slot: response.context.slot,
        timestamp: Some(Utc::now()),
        logs: Some(response.value.logs),
    };

    Ok(solana_tx)
}
