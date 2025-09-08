use crate::error::TransactionParsingError;
use solana_types::solana_types::SolanaTransaction;

pub fn is_log_emmitted(
    tx: &SolanaTransaction,
    op_code: u32,
    out_msg_log_index: usize,
) -> Result<bool, TransactionParsingError> {
    // Ok(Some(tx)
    //     .and_then(|tx| tx.in_msg.as_ref())
    //     .and_then(|in_msg| in_msg.opcode)
    //     .filter(|opcode| *opcode == op_code)
    //     .and_then(|_| tx.out_msgs.get(out_msg_log_index))
    //     .map(|msg| msg.destination.is_none())
    //     .unwrap_or(false))
    Err(TransactionParsingError::Generic(
        "not implemented".to_string(),
    ))
}
pub fn signature_to_message_id(signature: &str) -> Result<String, TransactionParsingError> {
    let bytes = bs58::decode(signature)
        .into_vec()
        .map_err(|e| TransactionParsingError::Generic(e.to_string()))?;
    Ok(format!("0x{}", hex::encode(bytes).to_lowercase()))
}
