use crate::error::TONRpcError::DataError;
use crate::error::{GasError, TONRpcError, TransactionParsingError};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use num_bigint::BigUint;
use relayer_base::price_view::PriceViewTrait;
use rust_decimal::Decimal;
use std::str::FromStr;
use ton_types::ton_types::Transaction;

pub fn is_log_emmitted(
    tx: &Transaction,
    op_code: u32,
    out_msg_log_index: usize,
) -> Result<bool, TransactionParsingError> {
    Ok(Some(tx)
        .and_then(|tx| tx.in_msg.as_ref())
        .and_then(|in_msg| in_msg.opcode)
        .filter(|opcode| *opcode == op_code)
        .and_then(|_| tx.out_msgs.get(out_msg_log_index))
        .map(|msg| msg.destination.is_none())
        .unwrap_or(false))
}

pub fn hash_to_message_id(hash: &str) -> Result<String, TONRpcError> {
    let hash = BASE64_STANDARD
        .decode(hash)
        .map_err(|e| DataError(e.to_string()))?;
    Ok(format!("0x{}", hex::encode(hash).to_lowercase()))
}

pub async fn convert_jetton_to_native<PV>(
    minter: String,
    amount: &BigUint,
    price_view: &PV,
) -> Result<BigUint, GasError>
where
    PV: PriceViewTrait,
{
    let coin_pair = format!("{}/USD", minter);
    let coin_to_usd = price_view
        .get_price(&coin_pair)
        .await
        .map_err(|err| GasError::ConversionError(err.to_string()))?;
    let ton_to_usd = price_view
        .get_price("TON/USD")
        .await
        .map_err(|err| GasError::ConversionError(err.to_string()))?;

    let amount = Decimal::from_str(&amount.to_string())
        .map_err(|e| GasError::ConversionError(format!("Invalid amount: {}", e)))?;
    let result = amount * coin_to_usd / ton_to_usd;
    let result = result.round();

    BigUint::from_str(&result.to_string()).map_err(|err| GasError::ConversionError(err.to_string()))
}
