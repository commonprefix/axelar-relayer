use std::sync::Arc;

use crate::error::RefundManagerError;
use crate::gmp_api::gmp_types::RefundTask;
use crate::includer::RefundManager;
use crate::utils::{extract_and_decode_memo, parse_message_from_context};
use libsecp256k1::{PublicKey, SecretKey};
use tracing::debug;
use xrpl_binary_codec::serialize;
use xrpl_binary_codec::sign::sign_transaction;
use xrpl_types::{AccountId, Amount, Blob, Memo, PaymentTransaction};

use super::client::XRPLClient;
pub struct XRPLRefundManager {
    client: Arc<XRPLClient>,
    account_id: AccountId,
    secret_key: SecretKey,
    public_key: PublicKey,
}

impl<'a> XRPLRefundManager {
    pub fn new(
        client: Arc<XRPLClient>,
        address: String,
        secret: String,
    ) -> Result<Self, RefundManagerError> {
        let account_id = AccountId::from_address(&address)
            .map_err(|e| RefundManagerError::GenericError(format!("Invalid address: {}", e)))?;

        let secret_bytes = hex::decode(&secret)
            .map_err(|e| RefundManagerError::GenericError(format!("Hex decode error: {}", e)))?;

        let secret_key = SecretKey::parse_slice(&secret_bytes).map_err(|err| {
            RefundManagerError::GenericError(format!("Invalid secret key: {:?}", err))
        })?;

        let public_key = PublicKey::from_secret_key(&secret_key);

        debug!("Creating refund manager with address: {}", address);
        Ok(Self {
            client,
            account_id,
            secret_key,
            public_key,
        })
    }
}

impl RefundManager for XRPLRefundManager {
    async fn build_refund_tx(
        &self,
        recipient: String,
        drops: String,
        refund_id: &str,
    ) -> Result<Option<(String, String, String)>, RefundManagerError> {
        let pre_fee_amount_drops = drops.parse::<u64>().map_err(|e| {
            RefundManagerError::GenericError(format!("Invalid drops amount '{}': {}", drops, e))
        })?;

        let pre_fee_amount = Amount::drops(pre_fee_amount_drops).map_err(|e| {
            RefundManagerError::GenericError(format!("Failed to parse amount: {}", e))
        })?;

        let recipient_account = AccountId::from_address(&recipient).map_err(|e| {
            RefundManagerError::GenericError(format!("Invalid recipient address: {}", e))
        })?;

        let mut tx = PaymentTransaction::new(self.account_id, pre_fee_amount, recipient_account);

        tx.common.memos = vec![Memo {
            memo_data: Blob::from_hex(&hex::encode_upper(refund_id)).unwrap(),
            memo_format: None,
            memo_type: Blob::from_hex(&hex::encode_upper("refund_id")).unwrap(),
        }];

        self.client
            .inner()
            .prepare_transaction(&mut tx.common)
            .await
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

        let fee = tx
            .common
            .fee
            .ok_or_else(|| RefundManagerError::GenericError("Fee not set".to_string()))?;

        let actual_refund_amount = pre_fee_amount_drops as i64 - fee.drops() as i64;

        if actual_refund_amount <= 0 {
            return Ok(None);
        }

        tx.amount = Amount::drops(actual_refund_amount as u64).map_err(|e| {
            RefundManagerError::GenericError(format!("Failed to parse amount: {}", e))
        })?;

        sign_transaction(&mut tx, &self.public_key, &self.secret_key)
            .map_err(|e| RefundManagerError::GenericError(format!("Sign error: {}", e)))?;

        let tx_bytes = serialize::serialize(&tx)
            .map_err(|e| RefundManagerError::GenericError(format!("Serialization error: {}", e)))?;

        Ok(Some((
            hex::encode_upper(tx_bytes),
            actual_refund_amount.to_string(),
            fee.drops().to_string(),
        )))
    }

    async fn is_refund_processed(
        &self,
        refund_task: &RefundTask,
        refund_id: &str,
    ) -> Result<bool, RefundManagerError> {
        let recipient_account = AccountId::from_address(&refund_task.task.refund_recipient_address)
            .map_err(|e| {
                RefundManagerError::GenericError(format!("Invalid recipient address: {}", e))
            })?;

        let message = parse_message_from_context(&refund_task.common.meta)
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;
        let tx_id = message.tx_id().to_string();

        // Get the ledger index of the associated payment transaction
        let tx = self
            .client
            .get_transaction_by_id(tx_id)
            .await
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

        let ledger_index = tx.common().ledger_index.ok_or_else(|| {
            RefundManagerError::GenericError("Ledger index not found".to_string())
        })?;
        let transactions = self
            .client
            .get_transactions_for_account(&recipient_account, ledger_index)
            .await
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

        // iterate on all transactions and check the memos
        for tx in transactions {
            let refund_memo = extract_and_decode_memo(&tx.common().memos, "refund")
                .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

            if refund_memo == refund_id {
                return Ok(true);
            }
        }

        Ok(false)
    }
}
