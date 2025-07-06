/*!

Broadcaster implementation for TON. Listens to GATEWAY_TX (essentially APPROVE messages) and REFUND.

# Note

Relayer code assumes there is one message per transaction. This might not be a safe assumption
and broadcaster should potentially be returning a vector of BroadcastResults.

# TODO

- Actually calculate approve_message_value
- Implement refunds
- Implement Transaction Types for TON (?)
- Handle multiple messages per transaction.
- The inner logic will probably be refactored as soon as its reused
- Move MockQueryIdWrapper to mockall
- Check that rest api is getting a correct request
- We are always releasing wallet twice it seems
- Cleanup any unwrap's

*/

use super::client::{RestClient, V3MessageResponse};
use crate::approve_message::ApproveMessages;
use crate::high_load_query_id_db_wrapper::HighLoadQueryIdWrapper;
use crate::out_action::out_action;
use crate::relayer_execute_message::RelayerExecuteMessage;
use crate::ton_wallet_high_load_v3::TonWalletHighLoadV3;
use crate::wallet_manager::WalletManager;
use base64::engine::general_purpose;
use base64::Engine;
use num_bigint::BigUint;
use relayer_base::error::BroadcasterError::RPCCallFailed;
use relayer_base::gmp_api::gmp_types::ExecuteTaskFields;
use relayer_base::{
    error::BroadcasterError,
    includer::{BroadcastResult, Broadcaster},
};
use std::sync::Arc;
use tonlib_core::tlb_types::block::out_action::OutAction;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;
use tracing::error;

pub struct TONBroadcaster {
    wallet_manager: Arc<WalletManager>,
    query_id_wrapper: Arc<dyn HighLoadQueryIdWrapper>,
    client: Arc<dyn RestClient>,
    gateway_address: TonAddress,
    internal_message_value: u32,
    chain_name: String,
}

impl TONBroadcaster {
    pub fn new(
        wallet_manager: Arc<WalletManager>,
        client: Arc<dyn RestClient>,
        query_id_wrapper: Arc<dyn HighLoadQueryIdWrapper>,
        gateway_address: TonAddress,
        internal_message_value: u32,
        chain_name: String,
    ) -> error_stack::Result<Self, BroadcasterError> {
        Ok(TONBroadcaster {
            wallet_manager,
            client,
            query_id_wrapper,
            gateway_address,
            internal_message_value,
            chain_name,
        })
    }

    async fn send_to_chain(&self, wallet: &TonWalletHighLoadV3, actions: Vec<OutAction>) -> Result<V3MessageResponse, BroadcasterError> {
        let internal_message_value: BigUint = BigUint::from(self.internal_message_value);

        let query_id = self
            .query_id_wrapper
            .next(wallet.address.to_string().as_str(), wallet.timeout)
            .await
            .map_err(|e| {
                BroadcasterError::GenericError(format!("Query Id acquiring failed: {:?}", e))
            })?;

        let outgoing_message =
            wallet.outgoing_message(actions, query_id.query_id().await, internal_message_value);

        let tx = outgoing_message.serialize(true).unwrap();
        let boc = general_purpose::STANDARD.encode(&tx);

        self.client.post_v3_message(boc).await.map_err(|e| RPCCallFailed(e.to_string()))
    }

    // async fn store_messages_to_cache(&self, approve_messages: Vec<ApproveMessage>) {
    //     for approve_message in approve_messages {
    //         let msg = approve_message.clone();
    //         let cc_id = router_api::CrossChainId::new(approve_message.source_chain, approve_message.message_id).unwrap();
    //         let payload_value = PayloadCacheValue {
    //             message: GatewayV2Message {
    //                 message_id: msg.message_id,
    //                 source_chain: msg.source_chain,
    //             },
    //             payload: "".to_string(),
    //         };
    //         self.payload_cache.store(cc_id, payload_value).await.unwrap()
    //     }
    // }
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
        let approve_message_value: BigUint = BigUint::from(2_000_000_000u32); // TODO: We will need to calculate this

        let actions: Vec<OutAction> = vec![out_action(
            tx_blob.as_str(),
            approve_message_value.clone(),
            self.gateway_address.clone(),
        )];
        let wallet = self.wallet_manager.acquire().await.map_err(|e| {
            BroadcasterError::GenericError(format!("Wallet acquire failed: {:?}", e))
        })?;

        let result = (|| async {
            let res = self.send_to_chain(wallet, actions.clone()).await;
            let (tx_hash, status) = match res {
                Ok(response) => (response.message_hash, Ok(())),
                Err(err) => (String::new(), Err(err)),
            };

            Ok(BroadcastResult {
                transaction: TONTransaction,
                tx_hash,
                message_id: Some(message.message_id.clone()),
                source_chain: Some(message.source_chain.clone()),
                status,
                clear_payload_cache_on_success: false,
            })
        })()
        .await;

        self.wallet_manager.release(wallet).await;

        result
    }

    async fn broadcast_refund(&self, _tx_blob: String) -> Result<String, BroadcasterError> {
        todo!();
    }

    async fn broadcast_execute_message(
        &self,
        message: ExecuteTaskFields,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        let destination_address: TonAddress =
            message.message.destination_address.parse().map_err(|e| {
                BroadcasterError::GenericError(format!("TonAddressParseError: {:?}", e))
            })?;

        let decoded_bytes = general_purpose::STANDARD
            .decode(message.payload)
            .map_err(|e| {
                BroadcasterError::GenericError(format!("Failed decoding payload: {:?}", e))
            })?;

        let hex_payload = hex::encode(decoded_bytes);

        let message_id = message.message.message_id;
        let source_chain = message.message.source_chain;

        let wallet = self.wallet_manager.acquire().await.map_err(|e| {
            error!("Error acquiring wallet: {:?}", e);
            BroadcasterError::GenericError(format!("Wallet acquire failed: {:?}", e))
        })?;

        let result = (|| async {
            let relayer_execute_msg = RelayerExecuteMessage::new(
                message_id.clone(),
                source_chain.clone(),
                message.message.source_address,
                self.chain_name.clone(),
                destination_address,
                hex_payload,
                wallet.address.clone(),
            );

            let boc = relayer_execute_msg
                .to_cell()
                .to_boc_hex(true)
                .map_err(|e| {
                    BroadcasterError::GenericError(format!(
                        "Failed to serialize relayer execute message: {:?}",
                        e
                    ))
                })?;

            let execute_message_value: BigUint = BigUint::from(2_000_000_000u32); // We will need to calculate this

            let actions: Vec<OutAction> = vec![out_action(
                &boc,
                execute_message_value.clone(),
                self.gateway_address.clone(),
            )];

            let res = self.send_to_chain(wallet, actions.clone()).await;
            let (tx_hash, status) = match res {
                Ok(response) => (response.message_hash, Ok(())),
                Err(err) => (String::new(), Err(err)),
            };

            Ok(BroadcastResult {
                transaction: TONTransaction,
                tx_hash,
                message_id: Some(message_id.clone()),
                source_chain: Some(source_chain.clone()),
                status,
                clear_payload_cache_on_success: false,
            })
        })()
        .await;

        self.wallet_manager.release(wallet).await;

        result
    }
}

#[cfg(test)]
mod tests {
    use crate::broadcaster::{TONBroadcaster, TONTransaction};
    use crate::client::{MockRestClient, V3MessageResponse};
    use crate::high_load_query_id::HighLoadQueryId;
    use crate::high_load_query_id_db_wrapper::{
        HighLoadQueryIdWrapper, HighLoadQueryIdWrapperError,
    };
    use crate::wallet_manager::wallet_manager_tests::load_wallets;
    use base64::prelude::BASE64_STANDARD;
    use base64::Engine;
    use relayer_base::error::BroadcasterError;
    use relayer_base::gmp_api::gmp_types::{Amount, ExecuteTaskFields, GatewayV2Message};
    use relayer_base::includer::{BroadcastResult, Broadcaster};
    use std::str::FromStr;
    use std::sync::Arc;
    use tonlib_core::TonAddress;

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
        let mut client = MockRestClient::new();
        client.expect_post_v3_message().returning(|_| {
            Ok(V3MessageResponse {
                message_hash: "abc".to_string(),
                message_hash_norm: "ABC".to_string(),
            })
        });

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
            chain_name: "ton2".to_string(),
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
            clear_payload_cache_on_success: false,
        };

        let unwrapped = res.unwrap();

        assert_eq!(unwrapped.tx_hash, good.tx_hash);
        assert_eq!(unwrapped.message_id, good.message_id);
        assert_eq!(unwrapped.source_chain, good.source_chain);
    }

    #[tokio::test]
    async fn test_broadcast_prover_message_invalid_input() {
        let mut client = MockRestClient::new();
        client.expect_post_v3_message().returning(|_| {
            Ok(V3MessageResponse {
                message_hash: "abc".to_string(),
                message_hash_norm: "ABC".to_string(),
            })
        });
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
            chain_name: "ton2".to_string(),
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

    #[tokio::test]
    async fn test_broadcast_execute_message() {
        let mut client = MockRestClient::new();
        client.expect_post_v3_message().returning(|_| {
            Ok(V3MessageResponse {
                message_hash: "abc".to_string(),
                message_hash_norm: "ABC".to_string(),
            })
        });

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
            chain_name: "ton2".to_string(),
        };

        let execute_task = ExecuteTaskFields {
            message: GatewayV2Message {
                message_id: "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58-1".to_string(),
                source_chain: "avalanche-fuji".to_string(),
                destination_address: "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930".to_string(),
                source_address: "ton2".to_string(),
                payload_hash: "aea6524367000fb4a0aa20b1d4f63daad1ed9e9df70=".to_string()
            },
            payload: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gcmVsYXllciEAAAAAAAAAAAAAAAAA".to_string(),
            available_gas_balance: Amount { token_id: None, amount: "0".to_string() },
        };

        let res = broadcaster.broadcast_execute_message(execute_task).await;
        assert!(res.is_ok());

        let good = BroadcastResult {
            transaction: TONTransaction,
            tx_hash: "abc".to_string(),
            message_id: Some(
                "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58-1".to_string(),
            ),
            source_chain: Some("avalanche-fuji".to_string()),
            status: Ok(()),
            clear_payload_cache_on_success: false,
        };

        let unwrapped = res.unwrap();

        assert_eq!(unwrapped.tx_hash, good.tx_hash);
        assert_eq!(unwrapped.message_id, good.message_id);
        assert_eq!(unwrapped.source_chain, good.source_chain);
    }
}
