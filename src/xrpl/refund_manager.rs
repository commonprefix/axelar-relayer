use std::sync::Arc;

use crate::config::NetworkConfig;
use crate::error::RefundManagerError;
use crate::gmp_api::gmp_types::RefundTask;
use crate::includer::RefundManager;
use crate::utils::{extract_and_decode_memo, parse_message_from_context};
use libsecp256k1::{PublicKey, SecretKey};
use rand::seq::SliceRandom;
use redis::{Commands, ExistenceCheck, SetExpiry, SetOptions};
use tracing::debug;
use xrpl_binary_codec::serialize;
use xrpl_binary_codec::sign::sign_transaction;
use xrpl_types::{AccountId, Amount, Blob, Memo, PaymentTransaction};

use super::client::XRPLClient;
pub struct XRPLRefundManager {
    client: Arc<XRPLClient>,
    redis_pool: r2d2::Pool<redis::Client>,
    config: NetworkConfig,
}

impl XRPLRefundManager {
    pub fn new(
        client: Arc<XRPLClient>,
        config: NetworkConfig,
        redis_pool: r2d2::Pool<redis::Client>,
    ) -> Result<Self, RefundManagerError> {
        Ok(Self {
            client,
            redis_pool,
            config,
        })
    }

    fn release_wallet_lock(&self, address: &str) -> Result<(), RefundManagerError> {
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

        let _: () = redis_conn
            .del(format!("includer_{}", address))
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

        debug!("Released wallet lock for {}", address);

        Ok(())
    }

    fn get_wallet_lock(&self) -> Result<(SecretKey, PublicKey, AccountId), RefundManagerError> {
        let mut redis_conn = self
            .redis_pool
            .get()
            .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

        let mut secrets: Vec<&str> = self.config.includer_secrets.split(",").collect();
        secrets.shuffle(&mut rand::rng());
        for (i, secret) in secrets.into_iter().enumerate() {
            let address = self
                .config
                .refund_manager_addresses
                .split(",")
                .nth(i)
                .ok_or(RefundManagerError::GenericError(format!(
                    "Can't find address on index: {}",
                    i
                )))?;

            let account_id = AccountId::from_address(address)
                .map_err(|e| RefundManagerError::GenericError(format!("Invalid address: {}", e)))?;

            let set_opts = SetOptions::default()
                .conditional_set(ExistenceCheck::NX)
                .with_expiration(SetExpiry::EX(60));
            let wallet_lock: bool = redis_conn
                .set_options(
                    format!("includer_{}", account_id.to_address()),
                    true,
                    set_opts,
                )
                .map_err(|e| RefundManagerError::GenericError(e.to_string()))?;

            if !wallet_lock {
                debug!("Wallet {} is already locked", address);
                continue;
            }

            let secret_bytes = hex::decode(secret).map_err(|e| {
                RefundManagerError::GenericError(format!("Hex decode error: {}", e))
            })?;

            let secret_key = SecretKey::parse_slice(&secret_bytes).map_err(|err| {
                RefundManagerError::GenericError(format!("Invalid secret key: {:?}", err))
            })?;

            let public_key = PublicKey::from_secret_key(&secret_key);

            debug!("Picked wallet {}", address);
            return Ok((secret_key, public_key, account_id));
        }

        Err(RefundManagerError::GenericError(
            "No available wallet in pool".to_string(),
        ))
    }

    async fn build_and_sign_tx(
        &self,
        recipient: String,
        drops: String,
        refund_id: &str,
        account_id: AccountId,
        public_key: PublicKey,
        secret_key: SecretKey,
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

        let mut tx = PaymentTransaction::new(account_id, pre_fee_amount, recipient_account);

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

        sign_transaction(&mut tx, &public_key, &secret_key)
            .map_err(|e| RefundManagerError::GenericError(format!("Sign error: {}", e)))?;

        let tx_bytes = serialize::serialize(&tx)
            .map_err(|e| RefundManagerError::GenericError(format!("Serialization error: {}", e)))?;

        Ok(Some((
            hex::encode_upper(tx_bytes),
            actual_refund_amount.to_string(),
            fee.drops().to_string(),
        )))
    }
}

impl RefundManager for XRPLRefundManager {
    async fn build_refund_tx(
        &self,
        recipient: String,
        drops: String,
        refund_id: &str,
    ) -> Result<Option<(String, String, String)>, RefundManagerError> {
        let (secret_key, public_key, account_id) = self.get_wallet_lock()?;

        let tx_result = self
            .build_and_sign_tx(
                recipient, drops, refund_id, account_id, public_key, secret_key,
            )
            .await;

        self.release_wallet_lock(&account_id.to_address())?;

        tx_result
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
            .get_transaction_by_id(tx_id.trim_start_matches("0x").to_owned())
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
            let refund_memo = extract_and_decode_memo(&tx.common().memos, "refund_id");

            if let Ok(refund_memo) = refund_memo {
                if refund_memo == refund_id {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }
}
