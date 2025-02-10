use base64::engine;
use libsecp256k1::{PublicKey, SecretKey};
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;
use xrpl_api::{SubmitRequest, TransactionResult};
use xrpl_binary_codec::serialize;
use xrpl_binary_codec::sign::sign_transaction;
use xrpl_types::PaymentTransaction;
use xrpl_types::{AccountId, Amount};

use crate::config::Config;
use crate::error::{BroadcasterError, ClientError, RefundManagerError};
use crate::gmp_api::GmpApi;
use crate::includer::{Broadcaster, Includer, RefundManager};

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const RPC_URL: &str = "https://s.devnet.rippletest.net:51234";

pub struct XRPLClient {}

impl XRPLClient {
    pub fn new_http_client(url: &str) -> Result<xrpl_http_client::Client, ClientError> {
        let http_client = reqwest::ClientBuilder::new()
            .connect_timeout(DEFAULT_RPC_TIMEOUT)
            .timeout(DEFAULT_RPC_TIMEOUT)
            .build()
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        Ok(xrpl_http_client::Client::builder()
            .base_url(url)
            .http_client(http_client)
            .build())
    }
}

pub struct XRPLRefundManager {
    client: Arc<xrpl_http_client::Client>,
    account_id: AccountId,
    secret_key: SecretKey,
    public_key: PublicKey,
}

impl<'a> XRPLRefundManager {
    fn new(
        client: Arc<xrpl_http_client::Client>,
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
    ) -> Result<Option<(String, String, String)>, RefundManagerError> {
        let pre_fee_amount_drops = drops.parse::<u64>().map_err(|e| {
            RefundManagerError::GenericError(format!("Invalid drops amount '{}': {}", drops, e))
        })?;

        let pre_fee_amount = Amount::drops(pre_fee_amount_drops).map_err(|e| {
            RefundManagerError::GenericError(format!("Failed to parse amount: {}", e.to_string()))
        })?;

        let recipient_account = AccountId::from_address(&recipient).map_err(|e| {
            RefundManagerError::GenericError(format!("Invalid recipient address: {}", e))
        })?;

        let mut tx = PaymentTransaction::new(self.account_id, pre_fee_amount, recipient_account);

        self.client
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
            RefundManagerError::GenericError(format!("Failed to parse amount: {}", e.to_string()))
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
}

pub struct XRPLBroadcaster {
    client: Arc<xrpl_http_client::Client>,
}

impl XRPLBroadcaster {
    fn new(client: Arc<xrpl_http_client::Client>) -> error_stack::Result<Self, BroadcasterError> {
        Ok(XRPLBroadcaster { client })
    }
}

impl Broadcaster for XRPLBroadcaster {
    async fn broadcast(&self, tx_blob: String) -> Result<String, BroadcasterError> {
        let req = SubmitRequest::new(tx_blob);
        let response = self
            .client
            .call(req)
            .await
            .map_err(|e| BroadcasterError::RPCCallFailed(e.to_string()))?;

        let engine_result = serde_json::to_string(&response.engine_result).unwrap();
        if engine_result.starts_with("tes")
            || engine_result.starts_with("tec")
            || response.engine_result == TransactionResult::tefALREADY
        {
            let tx_hash = response.tx_json.common().hash.as_ref().ok_or_else(|| {
                BroadcasterError::RPCCallFailed("Transaction hash not found".to_string())
            })?;
            Ok(tx_hash.clone())
        } else {
            Err(BroadcasterError::RPCCallFailed(format!(
                "Transaction failed: {}",
                response.engine_result_message
            )))
        }
    }
}

pub struct XrplIncluder {}

impl XrplIncluder {
    pub async fn new<'a>(
        config: Config,
        gmp_api: Arc<GmpApi>,
    ) -> error_stack::Result<
        Includer<XRPLBroadcaster, Arc<xrpl_http_client::Client>, XRPLRefundManager>,
        BroadcasterError,
    > {
        let client =
            Arc::new(XRPLClient::new_http_client(RPC_URL).map_err(|e| {
                error_stack::report!(BroadcasterError::GenericError(e.to_string()))
            })?);

        let broadcaster = XRPLBroadcaster::new(Arc::clone(&client))
            .map_err(|e| e.attach_printable("Failed to create XRPLBroadcaster"))?;

        let refund_manager = XRPLRefundManager::new(
            Arc::clone(&client),
            config.refund_manager_address,
            config.includer_secret,
        )
        .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?;

        let includer = Includer {
            chain_client: client,
            broadcaster,
            refund_manager,
            gmp_api,
        };

        Ok(includer)
    }
}
