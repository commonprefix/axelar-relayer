/*!

Broadcaster implementation for TON. Listens to GATEWAY_TX (essentially APPROVE messages) and REFUND.

# Note

Relayer code assumes there is one message per transaction. This might not be a safe assumption
and broadcaster should potentially be returning a vector of BroadcastResults.

# TODO

- Ensure that even if we fail when sending we properly release the wallet. E.g. if POST fails, we
should release the wallet.
- Actually calculate approve_message_value
- Implement refunds
- Implement Transaction Types for TON
- Handle multiple messages per transaction.
- The inner logic will probably be refactored as soon as its reused

*/

use super::client::RestClient;
use crate::approve_message::ApproveMessages;
use crate::high_load_query_id_db_wrapper::HighLoadQueryIdWrapper;
use crate::out_action::out_action;
use crate::wallet_manager::WalletManager;
use base64::engine::general_purpose;
use base64::Engine;
use num_bigint::BigUint;
use relayer_base::error::BroadcasterError::RPCCallFailed;
use relayer_base::{
    error::BroadcasterError,
    includer::{BroadcastResult, Broadcaster},
};
use std::sync::Arc;
use tonlib_core::tlb_types::block::out_action::OutAction;
use tonlib_core::TonAddress;
use tracing::error;

pub struct TONBroadcaster {
    wallet_manager: Arc<WalletManager>,
    query_id_wrapper: Arc<dyn HighLoadQueryIdWrapper>,
    client: Arc<dyn RestClient>,
    gateway_address: TonAddress,
    internal_message_value: u32,
}

impl TONBroadcaster {
    pub fn new(
        wallet_manager: Arc<WalletManager>,
        client: Arc<dyn RestClient>,
        query_id_wrapper: Arc<dyn HighLoadQueryIdWrapper>,
        gateway_address: TonAddress,
        internal_message_value: u32,
    ) -> error_stack::Result<Self, BroadcasterError> {
        Ok(TONBroadcaster {
            wallet_manager,
            client,
            query_id_wrapper,
            gateway_address,
            internal_message_value,
        })
    }
}

pub struct TONTransaction;

impl Broadcaster for TONBroadcaster {
    type Transaction = TONTransaction;

    async fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        let approve_messages = ApproveMessages::from_boc_hex(&tx_blob)
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;

        let message = &approve_messages.approve_messages[0];
        let internal_message_value: BigUint = BigUint::from(self.internal_message_value);
        let approve_message_value: BigUint = BigUint::from(2_000_000_000u32); // We will need to calculate this

        let actions: Vec<OutAction> = vec![out_action(
            tx_blob.as_str(),
            approve_message_value.clone(),
            self.gateway_address.clone(),
        )];
        let wallet = self.wallet_manager.acquire().await.map_err(|e| {
            error!("Error acquiring wallet: {:?}", e);
            BroadcasterError::GenericError(format!("Wallet acquire failed: {:?}", e))
        })?;
        let query_id = self
            .query_id_wrapper
            .next(wallet.address.to_string().as_str(), wallet.timeout)
            .await
            .map_err(|e| {
                error!("Query Id acquiring failed: {:?}", e);
                BroadcasterError::GenericError(format!("Query Id acquiring failed: {:?}", e))
            })?;
        let outgoing_message =
            wallet.outgoing_message(actions, query_id.query_id().await, internal_message_value);

        let tx = outgoing_message.serialize(true).unwrap();
        let boc = general_purpose::STANDARD.encode(&tx);
        let response = self
            .client
            .post_v3_message(boc)
            .await
            .map_err(|e| RPCCallFailed(e.to_string()))?;
        self.wallet_manager.release(wallet).await;

        Ok(BroadcastResult {
            transaction: TONTransaction,
            tx_hash: response.message_hash,
            message_id: Some(message.message_id.clone()),
            source_chain: Some(message.source_chain.clone()),
            status: Ok(()),
        })
    }

    async fn broadcast_refund(&self, _tx_blob: String) -> Result<String, BroadcasterError> {
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use crate::broadcaster::{TONBroadcaster, TONTransaction};
    use crate::client::{RestClient, V3MessageResponse};
    use crate::high_load_query_id::HighLoadQueryId;
    use crate::high_load_query_id_db_wrapper::{
        HighLoadQueryIdWrapper, HighLoadQueryIdWrapperError,
    };
    use crate::wallet_manager::wallet_manager_tests::load_wallets;
    use relayer_base::error::{BroadcasterError, ClientError};
    use relayer_base::includer::{BroadcastResult, Broadcaster};
    use std::str::FromStr;
    use std::sync::Arc;
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use tonlib_core::TonAddress;

    struct MockTONClient;

    #[async_trait::async_trait]
    impl RestClient for MockTONClient {
        async fn post_v3_message(
            &self,
            _tx_blob: String,
        ) -> Result<V3MessageResponse, ClientError> {
            Ok(V3MessageResponse {
                message_hash: "abc".to_string(),
                message_hash_norm: "ABC".to_string(),
            })
        }
    }

    struct MockQueryIdWrapper;

    #[async_trait::async_trait]
    impl HighLoadQueryIdWrapper for MockQueryIdWrapper {
        async fn next(
            &self,
            _address: &str,
            _timeout: u64,
        ) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError> {
            Ok(HighLoadQueryId::from_shift_and_bitnumber(0u32, 0u32)
                .await
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_broadcast_prover_message() {
        let client = MockTONClient;
        let wallet_manager = load_wallets().await;
        let query_id_wrapper = MockQueryIdWrapper;
        let gateway_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let internal_message_value = 1_000_000_000u32;

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            internal_message_value,
        };
        let approve_message = hex::encode(BASE64_STANDARD.decode("te6cckECDAEAAYsAAggAAAAoAQIBYYAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADf5gkADAQHABADi0LAAUYmshNOh1nWEdwB3eJHd51H6EH1kg3v2M30y32eQAAAAAAAAAAAAAAAAAAAAAQ+j+g0KWjWTaPqB9qQHuWZQn7IPz7x3xzwbprT1a85sjh0UlPlFU84LDdRcD4GZ6n6GJlEKKTlRW5QtlzKGrAsBAtAFBECeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwYHCAkAiDB4MTdmZDdkYTNkODE5Y2ZiYzQ2ZmYyOGYzZDgwOTgwNzcwZWMxYjgwZmQ3ZDFiMjI5Y2VjMzI1MTkzOWI5YjIzZi0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AgAKCwBAuHpKD2RLehhu5xoUVGNPcMIqYqyhprpna1F1wh1/2TAACHRvbjJLddsV").unwrap());

        let res = broadcaster
            .broadcast_prover_message(approve_message.to_string())
            .await;
        assert!(res.is_ok());

        let good = BroadcastResult {
            transaction: TONTransaction,
            tx_hash: "abc".to_string(),
            message_id: Some(
                "0x17fd7da3d819cfbc46ff28f3d80980770ec1b80fd7d1b229cec3251939b9b23f-1".to_string(),
            ),
            source_chain: Some("avalanche-fuji".to_string()),
            status: Ok(()),
        };

        let unwrapped = res.unwrap();

        assert_eq!(unwrapped.tx_hash, good.tx_hash);
        assert_eq!(unwrapped.message_id, good.message_id);
        assert_eq!(unwrapped.source_chain, good.source_chain);
    }

    #[tokio::test]
    async fn test_broadcast_prover_message_invalid_input() {
        let client = MockTONClient;
        let wallet_manager = load_wallets().await;
        let query_id_wrapper = MockQueryIdWrapper;
        let gateway_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let internal_message_value = 1_000_000_000u32;

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            internal_message_value,
        };

        // Invalid base64 string for BOC (non-decodable)
        let invalid_approve_message = "!!!invalid_base64_data###";

        let res = broadcaster
            .broadcast_prover_message(invalid_approve_message.to_string())
            .await;

        assert!(res.is_err());

        match res {
            Err(BroadcasterError::GenericError(e)) => {
                assert!(
                    e.contains("BocParsingError") || e.contains("BoC deserialization error"),
                    "Expected BoC deserialization error, got: {}",
                    e
                );
            }
            _other => panic!("Expected GenericError with BoC parsing issue"),
        }
    }
}
