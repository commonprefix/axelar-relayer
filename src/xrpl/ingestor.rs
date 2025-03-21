use base64::prelude::*;
use core::str;
use std::{collections::HashMap, str::FromStr, sync::Arc, vec};

use router_api::CrossChainId;
use tracing::{debug, warn};
use xrpl_amplifier_types::{
    msg::{WithPayload, XRPLMessage, XRPLProverMessage, XRPLUserMessage},
    types::{TxHash, XRPLAccountId, XRPLPaymentAmount, XRPLToken, XRPLTokenAmount},
};
use xrpl_api::{Memo, PaymentTransaction, Transaction, TxRequest};
use xrpl_gateway::msg::InterchainTransfer;

use crate::{
    config::Config,
    error::IngestorError,
    gmp_api::{
        gmp_types::{
            self, Amount, BroadcastRequest, CommonEventFields, ConstructProofTask, Event,
            GatewayV2Message, MessageExecutionStatus, Metadata, QueryRequest, ReactToWasmEventTask,
            VerifyTask,
        },
        GmpApi,
    },
    payload_cache::PayloadCacheClient,
    utils::{event_attribute, extract_from_xrpl_memo, extract_hex_xrpl_memo},
};

fn extract_memo(memos: &Option<Vec<Memo>>, memo_type: &str) -> Result<String, IngestorError> {
    extract_from_xrpl_memo(memos.clone(), memo_type).map_err(|e| {
        IngestorError::GenericError(format!("Failed to extract {} from memos: {}", memo_type, e))
    })
}

async fn build_xrpl_user_message(
    payload_cache: &PayloadCacheClient,
    payment: &PaymentTransaction,
) -> Result<WithPayload<XRPLUserMessage>, IngestorError> {
    let tx_hash = payment.common.hash.clone().ok_or_else(|| {
        IngestorError::GenericError("Payment transaction missing field 'hash'".to_owned())
    })?;

    let memos = &payment.common.memos;

    let destination_address = extract_memo(memos, "destination_address")?;
    let destination_chain = extract_memo(memos, "destination_chain")?;
    // let gas_fee_amount = extract_memo(memos, "gas_fee_amount")?;
    let amount = if let xrpl_api::Amount::Drops(amount) = payment.amount.clone() {
        XRPLPaymentAmount::Drops(
            amount.parse::<u64>().map_err(|_| {
                IngestorError::GenericError("Failed to parse amount as u64".to_owned())
            })?,
        )
    } else if let xrpl_api::Amount::Issued(issued_amount) = payment.amount.clone() {
        XRPLPaymentAmount::Issued(
            XRPLToken {
                issuer: issued_amount.issuer.try_into().map_err(|_| {
                    IngestorError::GenericError(
                        "Failed to parse issuer as XRPLAccountId".to_owned(),
                    )
                })?,
                currency: issued_amount.currency.try_into().map_err(|_| {
                    IngestorError::GenericError(
                        "Failed to parse currency as XRPLCurrency".to_owned(),
                    )
                })?,
            },
            XRPLTokenAmount::from_str(&issued_amount.value).map_err(|_| {
                IngestorError::GenericError("Failed to parse amount as XRPLTokenAmount".to_owned())
            })?,
        )
    } else {
        return Err(IngestorError::GenericError(
            "Payment amount must be either Drops or Issued".to_owned(),
        ));
    };
    let payload_hash_memo = extract_memo(memos, "payload_hash");
    let payload_memo = extract_memo(memos, "payload");

    // Payment transaction must not contain both 'payload' and 'payload_hash' memos.
    if payload_memo.is_ok() && payload_hash_memo.is_ok() {
        return Err(IngestorError::GenericError(
            "Payment transaction cannot have both 'payload' and 'payload_hash' memos".to_owned(),
        ));
    }

    let (payload, payload_hash) = if let Ok(payload_str) = payload_memo {
        // If we have 'payload', store it in the cache and receive the payload_hash.
        let hash = payload_cache
            .store_payload(&payload_str)
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to store payload in cache: {}", e))
            })?;
        (Some(payload_str), Some(hash))
    } else if let Ok(payload_hash_str) = payload_hash_memo {
        // If we have 'payload_hash', retrieve payload from the cache.
        let payload_retrieved =
            payload_cache
                .get_payload(&payload_hash_str)
                .await
                .map_err(|e| {
                    IngestorError::GenericError(format!("Failed to get payload from cache: {}", e))
                })?;
        (Some(payload_retrieved), Some(payload_hash_str))
    } else {
        (None, None)
    };

    let tx_id_bytes = hex::decode(&tx_hash)
        .map_err(|_| IngestorError::GenericError("Failed to hex-decode tx_id".into()))?;

    let source_address_bytes: XRPLAccountId = payment
        .common
        .account
        .clone()
        .try_into()
        .map_err(|e| IngestorError::GenericError(format!("Invalid source account: {:?}", e)))?;

    let destination_addr_bytes = hex::decode(destination_address).map_err(|e| {
        IngestorError::GenericError(format!("Failed to decode destination_address: {}", e))
    })?;

    let destination_chain_bytes = hex::decode(destination_chain).map_err(|e| {
        IngestorError::GenericError(format!("Failed to decode destination_chain: {}", e))
    })?;

    let destination_chain_str = str::from_utf8(&destination_chain_bytes).map_err(|e| {
        IngestorError::GenericError(format!("Invalid UTF-8 in destination_chain: {}", e))
    })?;

    let mut message_with_payload = WithPayload {
        message: XRPLUserMessage {
            tx_id: tx_id_bytes.as_slice().try_into().unwrap(),
            source_address: source_address_bytes.try_into().unwrap(),
            destination_address: destination_addr_bytes.try_into().unwrap(),
            destination_chain: destination_chain_str.try_into().unwrap(),
            payload_hash: None,
            amount,
        },
        payload: None,
    };

    if payload_hash.is_some() {
        let payload_hash_bytes = hex::decode(payload_hash.unwrap()).map_err(|e| {
            IngestorError::GenericError(format!("Failed to decode payload hash: {}", e))
        })?;
        message_with_payload.message.payload_hash =
            Some(payload_hash_bytes.try_into().map_err(|_| {
                IngestorError::GenericError("Invalid length of payload_hash bytes".into())
            })?)
    }

    if payload.is_some() {
        let payload_bytes = hex::decode(payload.unwrap()).unwrap();
        message_with_payload.payload = Some(payload_bytes.try_into().unwrap());
    }

    Ok(message_with_payload)
}

async fn xrpl_tx_from_hash(
    tx_hash: TxHash,
    client: &xrpl_http_client::Client,
) -> Result<Transaction, IngestorError> {
    let tx_request = TxRequest::new(&tx_hash.to_string()).binary(false);
    client
        .call(tx_request)
        .await
        .map_err(|e| {
            IngestorError::GenericError(format!("Failed to get transaction: {}", e.to_string()))
        })
        .map(|res| res.tx)
}

pub struct XrplIngestor {
    client: xrpl_http_client::Client,
    gmp_api: Arc<GmpApi>,
    config: Config,
    payload_cache: PayloadCacheClient,
}

fn parse_message_from_context(metadata: Option<Metadata>) -> Result<XRPLMessage, IngestorError> {
    let metadata = metadata
        .ok_or_else(|| IngestorError::GenericError("Verify task missing meta field".into()))?;

    let source_context = metadata.source_context.ok_or_else(|| {
        IngestorError::GenericError("Verify task missing source_context field".into())
    })?;

    let user_msg = source_context.get("user_message").ok_or_else(|| {
        IngestorError::GenericError("Verify task missing user_message in source_context".into())
    })?;

    Ok(serde_json::from_str(user_msg).unwrap())
}

impl XrplIngestor {
    pub fn new(gmp_api: Arc<GmpApi>, config: Config) -> Self {
        let client = xrpl_http_client::Client::builder()
            .base_url(&config.xrpl_rpc)
            .build();
        let payload_cache =
            PayloadCacheClient::new(&config.payload_cache, &config.payload_cache_auth_token);
        Self {
            gmp_api,
            config,
            client,
            payload_cache,
        }
    }

    pub async fn handle_transaction(&self, tx: Transaction) -> Result<Vec<Event>, IngestorError> {
        match tx.clone() {
            Transaction::Payment(payment) => {
                if payment.destination == self.config.xrpl_multisig {
                    self.handle_payment(payment).await
                } else if payment.common.account == self.config.xrpl_multisig {
                    // prover message
                    self.handle_prover_tx(tx).await
                } else {
                    Err(IngestorError::UnsupportedTransaction(
                        serde_json::to_string(&payment).unwrap(),
                    ))
                }
            }
            Transaction::TicketCreate(_) => self.handle_prover_tx(tx).await,
            Transaction::TrustSet(_) => self.handle_prover_tx(tx).await,
            Transaction::SignerListSet(_) => self.handle_prover_tx(tx).await,
            tx => {
                warn!(
                    "Unsupported transaction type: {}",
                    serde_json::to_string(&tx).unwrap()
                );
                Ok(vec![])
            }
        }
    }

    pub async fn handle_payment(
        &self,
        payment: PaymentTransaction,
    ) -> Result<Vec<Event>, IngestorError> {
        match build_xrpl_user_message(&self.payload_cache, &payment).await {
            Ok(user_message_with_payload) => {
                let call_event = self
                    .call_event_from_user_message(&user_message_with_payload)
                    .await?;
                let gas_credit_event = self.gas_credit_event_from_payment(&payment).await?;
                Ok(vec![call_event, gas_credit_event])
            }
            Err(e) => {
                warn!("Failed to build XRPL user message: {}", e);
                Ok(vec![])
            }
        }
    }

    pub async fn handle_prover_tx(&self, tx: Transaction) -> Result<Vec<Event>, IngestorError> {
        let unsigned_tx_hash = extract_memo(&tx.common().memos, "unsigned_tx_hash")?;
        let unsigned_tx_hash_bytes = hex::decode(&unsigned_tx_hash)
            .map_err(|e| IngestorError::GenericError(format!("Failed to decode tx hash: {}", e)))?;

        let execute_msg =
            xrpl_gateway::msg::ExecuteMsg::VerifyMessages(vec![XRPLMessage::ProverMessage(
                XRPLProverMessage {
                    tx_id: TxHash::new(
                        hex::decode(tx.common().hash.clone().ok_or_else(|| {
                            IngestorError::GenericError("Transaction missing hash".into())
                        })?)
                        .map_err(|_| {
                            IngestorError::GenericError("Invalid length of tx hash bytes".into())
                        })?
                        .try_into()
                        .map_err(|_| {
                            IngestorError::GenericError("Invalid length of tx hash bytes".into())
                        })?,
                    ),
                    unsigned_tx_hash: TxHash::new(unsigned_tx_hash_bytes.try_into().map_err(
                        |_| IngestorError::GenericError("Invalid length of tx hash bytes".into()),
                    )?),
                },
            )]);

        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize VerifyMessages: {}", e))
            })?);

        self.gmp_api
            .post_broadcast(self.config.xrpl_gateway_address.clone(), &request)
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
            })?;

        Ok(vec![])
    }

    async fn call_event_from_user_message(
        &self,
        xrpl_user_message_with_payload: &WithPayload<XRPLUserMessage>,
    ) -> Result<Event, IngestorError> {
        let xrpl_user_message = xrpl_user_message_with_payload.message.clone();

        let source_context = HashMap::from([(
            "user_message".to_owned(),
            serde_json::to_string(&XRPLMessage::UserMessage(xrpl_user_message.clone())).unwrap(),
        )]);

        let query = xrpl_gateway::msg::QueryMsg::InterchainTransfer {
            message_with_payload: xrpl_user_message_with_payload.clone(),
        };
        let request = QueryRequest::Generic(serde_json::to_value(&query).map_err(|e| {
            IngestorError::GenericError(format!("Failed to serialize InterchainTransfer: {}", e))
        })?);

        let response_body = self
            .gmp_api
            .post_query(self.config.xrpl_gateway_address.clone(), &request)
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to translate XRPL User Message to ITS Message: {}",
                    e
                ))
            })?;

        let interchain_transfer_response: InterchainTransfer = serde_json::from_str(&response_body)
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to parse ITS Message: {}", e))
            })?;

        let message_with_payload = interchain_transfer_response
            .message_with_payload
            .clone()
            .unwrap();

        let b64_payload = BASE64_STANDARD.encode(
            hex::decode(
                interchain_transfer_response
                    .message_with_payload
                    .clone()
                    .ok_or_else(|| {
                        IngestorError::GenericError("Failed to get payload.".to_owned())
                    })?
                    .payload
                    .to_string(),
            )
            .map_err(|e| IngestorError::GenericError(format!("Failed to decode payload: {}", e)))?,
        );

        Ok(Event::Call {
            common: CommonEventFields {
                r#type: "CALL".to_owned(),
                event_id: format!(
                    "{}-call",
                    xrpl_user_message.tx_id.to_string().to_lowercase()
                ),
                meta: Some(Metadata {
                    tx_id: None,
                    from_address: None,
                    finalized: None,
                    source_context: Some(source_context),
                }),
            },
            message: GatewayV2Message {
                message_id: message_with_payload.message.cc_id.message_id.to_lowercase(),
                source_chain: message_with_payload.message.cc_id.source_chain.to_string(),
                source_address: message_with_payload.message.source_address.to_string(),
                destination_address: message_with_payload.message.destination_address.to_string(),
                payload_hash: hex::encode(message_with_payload.message.payload_hash),
            },
            destination_chain: "axelar".to_owned(),
            payload: b64_payload,
        })
    }

    async fn gas_credit_event_from_payment(
        &self,
        payment: &PaymentTransaction,
    ) -> Result<Event, IngestorError> {
        let tx_hash = payment
            .common
            .hash
            .clone()
            .ok_or(IngestorError::GenericError(
                "Payment transaction missing field 'hash'".to_owned(),
            ))?;

        let gas_amount = 1000000; // TODO: get from memo
        Ok(Event::GasCredit {
            common: CommonEventFields {
                r#type: "GAS_CREDIT".to_owned(),
                event_id: format!("{}-gas", tx_hash.clone().to_lowercase()),
                meta: None,
            },
            message_id: format!("0x{}", tx_hash.to_lowercase()),
            refund_address: payment.common.account.clone(),
            payment: gmp_types::Amount {
                token_id: None,
                amount: gas_amount.to_string(),
            },
        })
    }

    pub async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        let xrpl_message = parse_message_from_context(task.common.meta)?;
        let user_message = match xrpl_message {
            XRPLMessage::UserMessage(user_message) => user_message,
            _ => {
                return Err(IngestorError::GenericError(
                    "Verify task message is not a UserMessage".to_owned(),
                ))
            }
        };

        let execute_msg =
            xrpl_gateway::msg::ExecuteMsg::VerifyMessages(vec![XRPLMessage::UserMessage(
                user_message.clone(),
            )]);
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize VerifyMessages: {}", e))
            })?);

        self.gmp_api
            .post_broadcast(self.config.xrpl_gateway_address.clone(), &request)
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
            })?;
        Ok(())
    }

    pub async fn prover_tx_routing_request(
        &self,
        tx: &Transaction,
    ) -> Result<(String, BroadcastRequest), IngestorError> {
        let tx_common = tx.common();

        let tx_hash = tx_common.hash.clone().ok_or_else(|| {
            IngestorError::GenericError("Transaction missing field 'hash'".into())
        })?;
        let unsigned_tx_hash = extract_memo(&tx_common.memos, "unsigned_tx_hash")?;

        let tx_hash_bytes = hex::decode(&tx_hash)
            .map_err(|e| IngestorError::GenericError(format!("Failed to decode tx hash: {}", e)))?;
        let unsigned_tx_hash_bytes = hex::decode(&unsigned_tx_hash)
            .map_err(|e| IngestorError::GenericError(format!("Failed to decode tx hash: {}", e)))?;

        let execute_msg = xrpl_multisig_prover::msg::ExecuteMsg::ConfirmProverMessage {
            prover_message: XRPLProverMessage {
                tx_id: TxHash::new(tx_hash_bytes.try_into().map_err(|_| {
                    IngestorError::GenericError("Invalid length of tx hash bytes".into())
                })?),
                unsigned_tx_hash: TxHash::new(unsigned_tx_hash_bytes.try_into().map_err(|_| {
                    IngestorError::GenericError("Invalid length of tx hash bytes".into())
                })?),
            },
        };
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize ConfirmTxStatus: {}", e))
            })?);
        Ok((self.config.xrpl_multisig_prover_address.clone(), request))
    }

    pub async fn user_message_routing_request(
        &self,
        user_message: XRPLUserMessage,
    ) -> Result<(String, BroadcastRequest), IngestorError> {
        let mut payload = None;
        if user_message.payload_hash.is_some() {
            let payload_string = Some(
                self.payload_cache
                    .get_payload(&hex::encode(&user_message.payload_hash.unwrap()))
                    .await
                    .map_err(|e| {
                        IngestorError::GenericError(format!(
                            "Failed to get payload from cache: {}",
                            e
                        ))
                    })?,
            );
            let payload_bytes = hex::decode(payload_string.unwrap()).unwrap();
            payload = Some(payload_bytes.try_into().unwrap());
        }
        let execute_msg = xrpl_gateway::msg::ExecuteMsg::RouteIncomingMessages(vec![WithPayload {
            message: user_message,
            payload,
        }]);
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to serialize RouteIncomingMessages: {}",
                    e
                ))
            })?);
        Ok((self.config.xrpl_gateway_address.clone(), request))
    }

    pub async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        let event_name = task.task.event.r#type.clone();

        // TODO: check the source contract of the event
        match event_name.as_str() {
            "wasm-quorum_reached" => {
                let content = event_attribute(&task.task.event, "content").ok_or_else(|| {
                    IngestorError::GenericError("QuorumReached event missing content".to_owned())
                })?;
                let xrpl_message: XRPLMessage = serde_json::from_str(&content).map_err(|e| {
                    IngestorError::GenericError(format!("Failed to parse XRPLMessage: {}", e))
                })?;
                let mut prover_tx = None;

                let (contract_address, request) = match xrpl_message.clone() {
                    XRPLMessage::UserMessage(msg) => {
                        debug!("Quorum reached for XRPLUserMessage: {:?}", msg);
                        self.user_message_routing_request(msg).await?
                    }
                    XRPLMessage::ProverMessage(prover_message) => {
                        debug!(
                            "Quorum reached for XRPLProverMessage: {:?}",
                            prover_message.tx_id
                        );
                        let tx = xrpl_tx_from_hash(prover_message.tx_id, &self.client).await?;
                        prover_tx = Some(tx);
                        self.prover_tx_routing_request(&prover_tx.as_ref().unwrap())
                            .await?
                    }
                };

                debug!("Broadcasting request: {:?}", request);
                self.gmp_api
                    .post_broadcast(contract_address, &request)
                    .await
                    .map_err(|e| {
                        IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
                    })?;

                self.handle_successful_routing(xrpl_message, prover_tx, task)
                    .await?;

                Ok(())
            }
            _ => Err(IngestorError::GenericError(format!(
                "Unknown event name: {}",
                event_name
            ))),
        }
    }

    pub async fn handle_successful_routing(
        &self,
        xrpl_message: XRPLMessage,
        prover_tx: Option<Transaction>,
        task: ReactToWasmEventTask,
    ) -> Result<(), IngestorError> {
        match xrpl_message {
            XRPLMessage::ProverMessage(_) => {
                match prover_tx.unwrap() {
                    Transaction::Payment(tx) => {
                        let tx_status: String = serde_json::from_str(
                            event_attribute(&task.task.event, "status")
                                .ok_or_else(|| {
                                    IngestorError::GenericError(
                                        "QuorumReached event for ProverMessage missing status"
                                            .to_owned(),
                                    )
                                })?
                                .as_str(),
                        )
                        .map_err(|e| {
                            IngestorError::GenericError(format!("Failed to parse status: {}", e))
                        })?;

                        let status = match tx_status.as_str() {
                            "succeeded_on_source_chain" => MessageExecutionStatus::SUCCESSFUL,
                            _ => MessageExecutionStatus::REVERTED,
                        };

                        let common = tx.common;
                        let message_id = extract_hex_xrpl_memo(common.memos.clone(), "message_id")
                            .map_err(|e| {
                                IngestorError::GenericError(format!(
                                    "Failed to extract message_id from memos: {}",
                                    e
                                ))
                            })?;
                        let source_chain =
                            extract_hex_xrpl_memo(common.memos.clone(), "source_chain").map_err(
                                |e| {
                                    IngestorError::GenericError(format!(
                                        "Failed to extract source_chain from memos: {}",
                                        e
                                    ))
                                },
                            )?;

                        // TODO: Don't send if the tx failed
                        // TODO: MessageExecuted could be moved earlier, right after broadcasting the message
                        let event = Event::MessageExecuted {
                            common: CommonEventFields {
                                r#type: "MESSAGE_EXECUTED".to_owned(),
                                event_id: common.hash.unwrap(),
                                meta: None,
                            },
                            message_id: message_id.to_string(),
                            source_chain: source_chain.to_string(),
                            status,
                            cost: Amount {
                                token_id: None,
                                amount: common.fee,
                            },
                        };
                        let events_response =
                            self.gmp_api.post_events(vec![event]).await.map_err(|e| {
                                IngestorError::GenericError(format!(
                                    "Failed to broadcast message: {}",
                                    e.to_string()
                                ))
                            })?;
                        let response =
                            events_response.get(0).ok_or(IngestorError::GenericError(
                                "Failed to get response from posting events".to_owned(),
                            ))?;
                        if response.status != "ACCEPTED" {
                            return Err(IngestorError::GenericError(format!(
                                "Failed to post event: {}",
                                response.error.clone().unwrap_or_default()
                            )));
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    pub async fn handle_construct_proof(
        &self,
        task: ConstructProofTask,
    ) -> Result<(), IngestorError> {
        let cc_id = CrossChainId::new(task.task.message.source_chain, task.task.message.message_id)
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to construct CrossChainId: {}", e))
            })?;

        let payload_bytes = BASE64_STANDARD.decode(&task.task.payload).map_err(|e| {
            IngestorError::GenericError(format!("Failed to decode task payload: {}", e))
        })?;

        let execute_msg = xrpl_multisig_prover::msg::ExecuteMsg::ConstructProof {
            cc_id: cc_id,
            payload: payload_bytes.into(),
        };

        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize ConstructProof: {}", e))
            })?);

        self.gmp_api
            .post_broadcast(self.config.xrpl_multisig_prover_address.clone(), &request)
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to broadcast message: {}",
                    e.to_string()
                ))
            })?;
        Ok(())
    }
}
