/*!

Broadcaster implementation for TON. Listens to GATEWAY_TX (essentially APPROVE messages) and REFUND.

# Note

Relayer code assumes there is one message per transaction. This might not be a safe assumption,
and broadcaster should potentially be returning a vector of BroadcastResults.

*/

use super::client::{RestClient, V3MessageResponse};
use crate::boc::approve_message::ApproveMessages;
use crate::boc::native_refund::NativeRefundMessage;
use crate::gas_estimator::GasEstimator;
use crate::high_load_query_id_db_wrapper::HighLoadQueryIdWrapper;
use crate::out_action::out_action;
use crate::relayer_execute_message::RelayerExecuteMessage;
use crate::ton_constants::REFUND_DUST;
use crate::ton_wallet_high_load_v3::TonWalletHighLoadV3;
use crate::wallet_manager::WalletManager;
use base64::engine::general_purpose;
use base64::Engine;
use num_bigint::BigUint;
use relayer_base::error::BroadcasterError::RPCCallFailed;
use relayer_base::gmp_api::gmp_types::{ExecuteTaskFields, RefundTaskFields};
use relayer_base::{
    error::BroadcasterError,
    includer::{BroadcastResult, Broadcaster},
};
use std::str::FromStr;
use std::sync::Arc;
use tonlib_core::tlb_types::block::out_action::OutAction;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::{TonAddress, TonHash};
use tracing::{debug, error, info, warn};

const HIGHLOAD_WALLET_ALREADY_EXECUTED: &str = "THROWIF 36";

pub struct TONBroadcaster<GE> {
    wallet_manager: Arc<WalletManager>,
    query_id_wrapper: Arc<dyn HighLoadQueryIdWrapper>,
    client: Arc<dyn RestClient>,
    gateway_address: TonAddress,
    gas_service_address: TonAddress,
    chain_name: String,
    gas_estimator: GE,
}

impl<GE: GasEstimator> TONBroadcaster<GE> {
    pub fn new(
        wallet_manager: Arc<WalletManager>,
        client: Arc<dyn RestClient>,
        query_id_wrapper: Arc<dyn HighLoadQueryIdWrapper>,
        gateway_address: TonAddress,
        gas_service_address: TonAddress,
        chain_name: String,
        gas_estimator: GE,
    ) -> error_stack::Result<Self, BroadcasterError> {
        Ok(TONBroadcaster {
            wallet_manager,
            client,
            query_id_wrapper,
            gateway_address,
            gas_service_address,
            chain_name,
            gas_estimator,
        })
    }

    async fn send_to_chain(
        &self,
        wallet: &TonWalletHighLoadV3,
        actions: Vec<OutAction>,
        retries_left: Option<u32>,
    ) -> Result<V3MessageResponse, BroadcasterError> {
        if let Some(1) = retries_left {
            error!("Last retry attempt for send_to_chain operation");
        }

        let query_id = self
            .query_id_wrapper
            .next(
                wallet.address.to_string().as_str(),
                wallet.timeout,
                retries_left.is_some(),
            )
            .await
            .map_err(|e| {
                BroadcasterError::GenericError(format!("Query Id acquiring failed: {e:?}"))
            })?;

        let outgoing_message = wallet
            .outgoing_message(
                &actions,
                query_id.query_id().await,
                BigUint::from(self.gas_estimator.highload_wallet_send(actions.len()).await),
            )
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;

        let tx = outgoing_message
            .serialize(true)
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;
        let boc = general_purpose::STANDARD.encode(&tx);

        debug!(
            "Sending boc: {:?} to post_v3_message with query_id: {:?}",
            boc, query_id
        );

        let result = self.client.post_v3_message(boc.clone()).await;

        match result {
            Ok(response) => Ok(response),
            Err(e) => {
                let error_str = e.to_string();
                // High load wallet "already executed" error
                if error_str.contains(HIGHLOAD_WALLET_ALREADY_EXECUTED) {
                    let retries = match retries_left {
                        Some(r) if r > 0 => Some(r - 1),
                        None => Some(10),
                        _ => None,
                    };

                    if retries.is_some() {
                        warn!(
                            "Encountered {} error, retrying. Retries left: {:?}",
                            HIGHLOAD_WALLET_ALREADY_EXECUTED, retries
                        );
                        // https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
                        return Box::pin(self.send_to_chain(wallet, actions, retries)).await;
                    }
                }
                Err(RPCCallFailed(error_str))
            }
        }
    }
}

pub struct TONTransaction;

impl<GE: GasEstimator> Broadcaster for TONBroadcaster<GE> {
    type Transaction = TONTransaction;

    async fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        let approve_messages = ApproveMessages::from_boc_hex(&tx_blob)
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;

        let message =
            approve_messages
                .approve_messages
                .first()
                .ok_or(BroadcasterError::GenericError(
                    "Missing approved message".to_string(),
                ))?;

        let approve_message_value: BigUint = BigUint::from(
            self.gas_estimator
                .approve_send(approve_messages.approve_messages.len())
                .await,
        );
        info!(
            "Sending approve message: message_id={}, source_chain={}",
            message.message_id, message.source_chain
        );

        let actions: Vec<OutAction> = vec![out_action(
            tx_blob.as_str(),
            approve_message_value,
            self.gateway_address.clone(),
        )
        .map_err(|e| BroadcasterError::GenericError(e.to_string()))?];

        let wallet =
            self.wallet_manager.acquire().await.map_err(|e| {
                BroadcasterError::GenericError(format!("Wallet acquire failed: {e:?}"))
            })?;

        let result = async {
            let res = self.send_to_chain(wallet, actions.clone(), None).await;
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
            })
        }
        .await;

        self.wallet_manager.release(wallet).await;

        result
    }

    async fn broadcast_refund(&self, _data: String) -> Result<String, BroadcasterError> {
        Ok(String::new())
    }

    async fn broadcast_execute_message(
        &self,
        message: ExecuteTaskFields,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError> {
        let destination_address: TonAddress =
            message.message.destination_address.parse().map_err(|e| {
                BroadcasterError::GenericError(format!("TonAddressParseError: {e:?}"))
            })?;

        let decoded_bytes = general_purpose::STANDARD
            .decode(message.payload.clone())
            .map_err(|e| {
                BroadcasterError::GenericError(format!("Failed decoding payload: {e:?}"))
            })?;

        let payload_len = decoded_bytes.len();

        let hex_payload = hex::encode(decoded_bytes.clone());

        let message_id = message.message.message_id;
        let source_chain = message.message.source_chain;

        let available_gas = u64::from_str(&message.available_gas_balance.amount).unwrap_or(0);
        let required_gas = self.gas_estimator.execute_estimate(payload_len).await;

        info!(
            "Considering execute message: message_id={}, source_chain={}, available_gas={}, required_gas={}, payload_len={}",
            message_id, source_chain, available_gas, required_gas, payload_len
        );
        if available_gas < required_gas {
            return Ok(BroadcastResult {
                transaction: TONTransaction,
                tx_hash: String::new(),
                message_id: Some(message_id),
                source_chain: Some(source_chain),
                status: Err(BroadcasterError::InsufficientGas(
                    "Cannot proceed to execute".to_string(),
                )),
            });
        }

        let wallet = self.wallet_manager.acquire().await.map_err(|e| {
            error!("Error acquiring wallet: {:?}", e);
            BroadcasterError::GenericError(format!("Wallet acquire failed: {e:?}"))
        })?;

        let result = async {
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
                .map_err(|e| BroadcasterError::GenericError(e.to_string()))?
                .to_boc_hex(true)
                .map_err(|e| {
                    BroadcasterError::GenericError(format!(
                        "Failed to serialize relayer execute message: {e:?}"
                    ))
                })?;

            let execute_message_value: BigUint =
                BigUint::from(self.gas_estimator.execute_send(payload_len).await);

            let actions: Vec<OutAction> = vec![out_action(
                &boc,
                execute_message_value.clone(),
                self.gateway_address.clone(),
            )
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?];

            let res = self.send_to_chain(wallet, actions.clone(), None).await;
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
            })
        }
        .await;

        self.wallet_manager.release(wallet).await;

        result
    }

    async fn broadcast_refund_message(
        &self,
        refund_task: RefundTaskFields,
    ) -> Result<String, BroadcasterError> {
        if refund_task.remaining_gas_balance.token_id.is_some() {
            return Err(BroadcasterError::GenericError(
                "Refund task with token_id is not supported".to_string(),
            ));
        }

        let cleaned_hash = refund_task
            .message
            .message_id
            .strip_prefix("0x")
            .unwrap_or(&refund_task.message.message_id);
        let tx_hash = TonHash::from_hex(cleaned_hash)
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?;

        let address = TonAddress::from_hex_str(&refund_task.refund_recipient_address)
            .map_err(|err| BroadcasterError::GenericError(err.to_string()))?;

        let original_amount = BigUint::from_str(&refund_task.remaining_gas_balance.amount)
            .map_err(|err| BroadcasterError::GenericError(err.to_string()))?;
        let gas_estimate = self.gas_estimator.native_gas_refund_estimate().await;

        info!(
            "Considering refund message: message_id={}, address={}, original_amount={}, gas_estimate={}",
            refund_task.message.message_id, address, refund_task.remaining_gas_balance.amount, gas_estimate
        );

        if original_amount < BigUint::from(gas_estimate) {
            info!(
                "Not enough balance to cover gas for refund: message_id={}",
                refund_task.message.message_id
            );
            return Err(BroadcasterError::InsufficientGas(
                "Not enough balance to cover gas for refund".to_string(),
            ));
        }

        let amount = original_amount - BigUint::from(gas_estimate);

        let native_refund = NativeRefundMessage::new(tx_hash, address, amount);

        let boc = native_refund
            .to_cell()
            .map_err(|e| BroadcasterError::GenericError(e.to_string()))?
            .to_boc_hex(true)
            .map_err(|e| {
                BroadcasterError::GenericError(format!(
                    "Failed to serialize relayer execute message: {e:?}"
                ))
            })?;

        let wallet = self.wallet_manager.acquire().await.map_err(|e| {
            error!("Error acquiring wallet: {e:?}");
            BroadcasterError::GenericError(format!("Wallet acquire failed: {e:?}"))
        })?;

        let result = async {
            let msg_value: BigUint = BigUint::from(REFUND_DUST);

            let actions: Vec<OutAction> =
                vec![
                    out_action(&boc, msg_value.clone(), self.gas_service_address.clone())
                        .map_err(|e| BroadcasterError::GenericError(e.to_string()))?,
                ];

            let res = self.send_to_chain(wallet, actions.clone(), None).await;
            let (tx_hash, _status) = match res {
                Ok(response) => (response.message_hash, Ok(())),
                Err(err) => (String::new(), Err(err)),
            };

            Ok(tx_hash)
        }
        .await;

        self.wallet_manager.release(wallet).await;

        result
    }
}

#[cfg(test)]
mod tests {
    use crate::broadcaster::{TONBroadcaster, TONTransaction};
    use crate::client::{MockRestClient, V3MessageResponse};
    use crate::gas_estimator::MockGasEstimator;
    use crate::high_load_query_id::HighLoadQueryId;
    use crate::high_load_query_id_db_wrapper::{
        HighLoadQueryIdWrapper, HighLoadQueryIdWrapperError,
    };
    use crate::wallet_manager::wallet_manager_tests::load_wallets;
    use base64::prelude::BASE64_STANDARD;
    use base64::Engine;
    use relayer_base::error::BroadcasterError;
    use relayer_base::gmp_api::gmp_types::{
        Amount, ExecuteTaskFields, GatewayV2Message, RefundTaskFields,
    };
    use relayer_base::includer::{BroadcastResult, Broadcaster};
    use std::str::FromStr;
    use std::sync::Arc;
    use tonlib_core::cell::Cell;
    use tonlib_core::tlb_types::tlb::TLB;
    use tonlib_core::TonAddress;

    struct MockQueryIdWrapper;

    #[async_trait::async_trait]
    impl HighLoadQueryIdWrapper for MockQueryIdWrapper {
        async fn next(
            &self,
            _address: &str,
            _timeout: u64,
            _force_shift_increase: bool,
        ) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError> {
            Ok(HighLoadQueryId::from_shift_and_bitnumber(0u32, 0u32)
                .await
                .unwrap())
        }
    }

    #[tokio::test]
    async fn test_broadcast_prover_message() {
        let mut client = MockRestClient::new();

        client.expect_post_v3_message().returning(move |_| {
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
        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let mut gas_estimator = MockGasEstimator::new();
        gas_estimator
            .expect_approve_send()
            .returning(|_| Box::pin(async { 42u64 }));
        gas_estimator
            .expect_highload_wallet_send()
            .returning(|_| Box::pin(async { 1024u64 }));

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
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
        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let gas_estimator = MockGasEstimator::new();

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
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
                    "Expected BoC deserialization error, got: {e}",
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
        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let mut gas_estimator = MockGasEstimator::new();
        gas_estimator
            .expect_execute_estimate()
            .returning(|_| Box::pin(async { 42u64 }));
        gas_estimator
            .expect_execute_send()
            .returning(|_| Box::pin(async { 42u64 }));
        gas_estimator
            .expect_highload_wallet_send()
            .returning(|_| Box::pin(async { 1024u64 }));

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
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
            available_gas_balance: Amount { token_id: None, amount: "84".to_string() },
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
        };

        let unwrapped = res.unwrap();

        assert!(unwrapped.status.is_ok());
        assert_eq!(unwrapped.tx_hash, good.tx_hash);
        assert_eq!(unwrapped.message_id, good.message_id);
        assert_eq!(unwrapped.source_chain, good.source_chain);
    }

    #[tokio::test]
    async fn test_broadcast_execute_message_not_enough_gas() {
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
        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let mut gas_estimator = MockGasEstimator::new();
        gas_estimator
            .expect_execute_estimate()
            .returning(|_| Box::pin(async { 42u64 }));
        gas_estimator
            .expect_highload_wallet_send()
            .returning(|_| Box::pin(async { 1024u64 }));

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
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
            available_gas_balance: Amount { token_id: None, amount: "11".to_string() },
        };

        let res = broadcaster.broadcast_execute_message(execute_task).await;
        assert!(res.is_ok());

        let unwrapped = res.unwrap();

        assert!(unwrapped.status.is_err());
        assert_eq!(
            unwrapped.status.err().unwrap().to_string(),
            "Insufficient gas: Cannot proceed to execute"
        );
    }

    #[tokio::test]
    async fn test_broadcast_refund_message() {
        let mut client = MockRestClient::new();
        client
            .expect_post_v3_message()
            .withf(|boc| {
                let cell = Cell::from_boc_b64(boc);
                cell.is_ok()
            })
            .returning(|_| {
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
        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let mut gas_estimator = MockGasEstimator::new();
        gas_estimator
            .expect_native_gas_refund_estimate()
            .returning(|| Box::pin(async { 42u64 }));
        gas_estimator
            .expect_highload_wallet_send()
            .returning(|_| Box::pin(async { 1024u64 }));

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
        };

        let refund_task = refund_task();

        let res = broadcaster.broadcast_refund_message(refund_task).await;
        assert!(res.is_ok());

        let unwrapped = res.unwrap();

        assert_eq!(unwrapped, "abc");
    }

    #[tokio::test]
    async fn test_broadcast_refund_message_refund_too_big() {
        let client = mock_rest_client();

        let wallet_manager = load_wallets().await;
        let query_id_wrapper = MockQueryIdWrapper;
        let gateway_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let mut gas_estimator = MockGasEstimator::new();
        gas_estimator
            .expect_native_gas_refund_estimate()
            .returning(|| Box::pin(async { 1000u64 }));
        gas_estimator
            .expect_highload_wallet_send()
            .returning(|_| Box::pin(async { 1000u64 }));

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
        };

        let refund_task = refund_task();

        let res = broadcaster.broadcast_refund_message(refund_task).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_send_to_chain_retry_on_throwif_36() {
        use crate::out_action::out_action;
        use base64::prelude::BASE64_STANDARD;
        use base64::Engine;
        use hex;
        use num_bigint::BigUint;
        use relayer_base::error::ClientError;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::{Arc, Mutex};

        struct TestQueryIdWrapper {
            // It allows us to keep track easily without making &self mutable.
            // The execution is sequential though.
            force_shift_increase_count: Arc<AtomicU32>,
        }

        #[async_trait::async_trait]
        impl HighLoadQueryIdWrapper for TestQueryIdWrapper {
            async fn next(
                &self,
                _address: &str,
                _timeout: u64,
                force_shift_increase: bool,
            ) -> Result<HighLoadQueryId, HighLoadQueryIdWrapperError> {
                if force_shift_increase {
                    self.force_shift_increase_count
                        .fetch_add(1, Ordering::SeqCst);
                }
                Ok(HighLoadQueryId::from_shift_and_bitnumber(0u32, 0u32)
                    .await
                    .unwrap())
            }
        }

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let mut client = MockRestClient::new();
        client.expect_post_v3_message().returning(move |_| {
            let mut count = call_count_clone.lock().unwrap();
            *count += 1;
            if *count <= 5 {
                // First call fails with THROWIF 36
                Err(ClientError::BadResponse(
                    "THROWIF 36 error occurred".to_string(),
                ))
            } else {
                // Subsequent calls succeed
                Ok(V3MessageResponse {
                    message_hash: "abc".to_string(),
                    message_hash_norm: "ABC".to_string(),
                })
            }
        });

        let wallet_manager = load_wallets().await;

        let force_shift_increase_count = Arc::new(AtomicU32::new(0));

        let query_id_wrapper = TestQueryIdWrapper {
            force_shift_increase_count: Arc::clone(&force_shift_increase_count),
        };

        let gateway_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();

        let gas_service_address = TonAddress::from_str(
            "0:0000000000000000000000000000000000000000000000000000000000000fff",
        )
        .unwrap();

        let mut gas_estimator = MockGasEstimator::new();
        gas_estimator
            .expect_highload_wallet_send()
            .returning(|_| Box::pin(async { 1024u64 }));

        let broadcaster = TONBroadcaster {
            wallet_manager: Arc::new(wallet_manager),
            query_id_wrapper: Arc::new(query_id_wrapper),
            client: Arc::new(client),
            gateway_address,
            gas_service_address,
            chain_name: "ton2".to_string(),
            gas_estimator,
        };

        let wallet = broadcaster.wallet_manager.acquire().await.unwrap();
        let approve_message = hex::encode(BASE64_STANDARD.decode("te6cckECDAEAAYsAAggAAAAoAQIBYYAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADf5gkADAQHABADi0LAAUYmshNOh1nWEdwB3eJHd51H6EH1kg3v2M30y32eQAAAAAAAAAAAAAAAAAAAAAQ+j+g0KWjWTaPqB9qQHuWZQn7IPz7x3xzwbprT1a85sjh0UlPlFU84LDdRcD4GZ6n6GJlEKKTlRW5QtlzKGrAsBAtAFBECeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwYHCAkAiDB4MTdmZDdkYTNkODE5Y2ZiYzQ2ZmYyOGYzZDgwOTgwNzcwZWMxYjgwZmQ3ZDFiMjI5Y2VjMzI1MTkzOWI5YjIzZi0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AgAKCwBAuHpKD2RLehhu5xoUVGNPcMIqYqyhprpna1F1wh1/2TAACHRvbjJLddsV").unwrap());
        let actions = vec![out_action(
            &approve_message,
            BigUint::from(100u32),
            broadcaster.gateway_address.clone(),
        )
        .unwrap()];
        let result = broadcaster.send_to_chain(wallet, actions, None).await;
        broadcaster.wallet_manager.release(wallet).await;
        assert!(result.is_ok());
        assert_eq!(*call_count.lock().unwrap(), 6);
        assert_eq!(force_shift_increase_count.load(Ordering::SeqCst), 5);
    }

    fn mock_rest_client() -> MockRestClient {
        let mut client = MockRestClient::new();
        client
            .expect_post_v3_message()
            .withf(|boc| {
                let cell = Cell::from_boc_b64(boc);
                cell.is_ok()
            })
            .returning(|_| {
                Ok(V3MessageResponse {
                    message_hash: "abc".to_string(),
                    message_hash_norm: "ABC".to_string(),
                })
            });
        client
    }

    fn refund_task() -> RefundTaskFields {
        RefundTaskFields {
            message: GatewayV2Message {
                message_id: "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58"
                    .to_string(),
                source_chain: "avalanche-fuji".to_string(),
                destination_address:
                    "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930".to_string(),
                source_address: "ton2".to_string(),
                payload_hash: "aea6524367000fb4a0aa20b1d4f63daad1ed9e9df70=".to_string(),
            },
            refund_recipient_address:
                "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7".to_string(),
            remaining_gas_balance: Amount {
                token_id: None,
                amount: "42".to_string(),
            },
        }
    }
}
