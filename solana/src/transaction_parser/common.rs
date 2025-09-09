use crate::error::TransactionParsingError;

pub fn signature_to_message_id(signature: &str) -> Result<String, TransactionParsingError> {
    let bytes = bs58::decode(signature)
        .into_vec()
        .map_err(|e| TransactionParsingError::Generic(e.to_string()))?;
    Ok(format!("0x{}", hex::encode(bytes).to_lowercase()))
}
