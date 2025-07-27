use crate::config::XRPLConfig;
use crate::utils::message_id_from_retry_task;
use crate::xrpl_transaction::{PgXrplTransactionModel, XrplTransaction, XrplTransactionStatus};
use axelar_wasm_std::{msg_id::HexTxHash, nonempty};
use base64::prelude::*;
use interchain_token_service::TokenId;
use regex::Regex;
use relayer_base::gmp_api::gmp_types::{CannotExecuteMessageReason, RetryTask, VerificationStatus};
use relayer_base::ingestor::IngestorTrait;
use relayer_base::models::task_retries::{PgTaskRetriesModel, TaskRetries};
use relayer_base::subscriber::ChainTransaction;
use relayer_base::utils::extract_from_xrpl_memo;
use relayer_base::{
    database::Database,
    error::{ITSTranslationError, IngestorError},
    gmp_api::{
        gmp_types::{
            self, Amount, BroadcastRequest, CommonEventFields, ConstructProofTask, Event,
            EventMetadata, GatewayV2Message, MessageExecutedEventMetadata, MessageExecutionStatus,
            QueryRequest, ReactToWasmEventTask, VerifyTask,
        },
    },
    models::Model,
    payload_cache::{PayloadCache, PayloadCacheValue},
    price_view::PriceView,
    utils::{
        convert_token_amount_to_drops, event_attribute, extract_and_decode_memo,
        extract_hex_xrpl_memo, parse_gas_fee_amount, parse_message_from_context,
        parse_payment_amount, xrpl_tx_from_hash,
    },
};
use router_api::{ChainNameRaw, CrossChainId};
use rust_decimal::Decimal;
use std::ops::Sub;
use std::{collections::HashMap, str::FromStr, sync::Arc, vec};
use tracing::{debug, error, info, warn};
use xrpl_amplifier_types::error::XRPLError;
use xrpl_amplifier_types::{
    msg::{
        WithCrossChainId, WithPayload, XRPLAddGasMessage, XRPLAddReservesMessage,
        XRPLCallContractMessage, XRPLInterchainTransferMessage, XRPLMessage, XRPLMessageType,
        XRPLProverMessage,
    },
    types::{XRPLAccountId, XRPLPaymentAmount},
};
use xrpl_api::Transaction;
use xrpl_api::{Memo, PaymentTransaction};
use xrpl_gateway::msg::{CallContract, InterchainTransfer, MessageWithPayload};
use relayer_base::gmp_api::GmpApiTrait;
use relayer_base::payload_cache::PayloadCacheTrait;

const MAX_TASK_RETRIES: i32 = 5;

pub struct XrplIngestorModels {
    pub xrpl_transaction_model: PgXrplTransactionModel,
    pub task_retries: PgTaskRetriesModel,
}

pub struct XrplIngestor<DB: Database, G: GmpApiTrait + Send + Sync + 'static> {
    client: xrpl_http_client::Client,
    gmp_api: Arc<G>,
    config: XRPLConfig,
    price_view: PriceView<DB>,
    payload_cache: PayloadCache<DB>,
    models: XrplIngestorModels,
}

impl<DB: Database, G: GmpApiTrait + Send + Sync + 'static> XrplIngestor<DB, G> {
    pub fn new(
        gmp_api: Arc<G>,
        config: XRPLConfig,
        price_view: PriceView<DB>,
        payload_cache: PayloadCache<DB>,
        models: XrplIngestorModels,
    ) -> Self {
        let client = xrpl_http_client::Client::builder()
            .base_url(&config.xrpl_rpc)
            .build();
        Self {
            gmp_api,
            config,
            client,
            price_view,
            payload_cache,
            models,
        }
    }
    pub async fn handle_payment(
        &self,
        payment: PaymentTransaction,
    ) -> Result<Vec<Event>, IngestorError> {
        match self.build_xrpl_message(&payment).await {
            Ok(message_with_payload) => match &message_with_payload.message {
                XRPLMessage::InterchainTransferMessage(_) | XRPLMessage::CallContractMessage(_) => {
                    self.handle_gmp_message(&message_with_payload).await
                }
                XRPLMessage::AddGasMessage(_) => {
                    self.handle_add_gas_message(&message_with_payload).await
                }
                XRPLMessage::AddReservesMessage(_) => {
                    self.handle_add_reserves_message(&message_with_payload)
                        .await
                }
                _ => Ok(vec![]),
            },
            Err(e) => {
                warn!("Failed to build XRPL user message: {}", e);
                Ok(vec![])
            }
        }
    }

    pub async fn handle_gmp_message(
        &self,
        xrpl_message_with_payload: &WithPayload<XRPLMessage>,
    ) -> Result<Vec<Event>, IngestorError> {

        let mut events = vec![];

        events.push(
            match self
                .call_event_from_message(xrpl_message_with_payload)
                .await
            {
                Ok(maybe_event) => match maybe_event {
                    Some(event) => event,
                    None => {
                        return Ok(vec![]);
                    }
                },
                Err(e) => return Err(e),
            },
        );

        let token_id = self
            .get_token_id(&xrpl_message_with_payload.message)
            .await?;

        events.push(
            self.gas_credit_event_from_payment(xrpl_message_with_payload, token_id)
                .await?,
        );

        if matches!(
            xrpl_message_with_payload.message,
            XRPLMessage::InterchainTransferMessage(_)
        ) {
            events.push(
                self.its_interchain_transfer_event(
                    xrpl_message_with_payload.message.clone(),
                    token_id,
                )
                .await?,
            );
        }

        Ok(events)
    }

    pub async fn verify_message(&self, message: &XRPLMessage) -> Result<(), IngestorError> {
        let execute_msg = xrpl_gateway::msg::ExecuteMsg::VerifyMessages(vec![message.clone()]);

        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize VerifyMessages: {}", e))
            })?);

        let verify_tx_hash = self
            .gmp_api
            .post_broadcast(
                self.config
                    .common_config
                    .axelar_contracts
                    .chain_gateway
                    .clone(),
                &request,
            )
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let xrpl_tx_hash = message.tx_id().tx_hash_as_hex_no_prefix();
        self.models
            .xrpl_transaction_model
            .update_verify_tx(&xrpl_tx_hash, &verify_tx_hash)
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        info!(
            "VerifyMessage({}) tx hash: {}",
            message.tx_id(),
            verify_tx_hash
        );

        Ok(())
    }

    pub async fn handle_add_gas_message(
        &self,
        xrpl_message_with_payload: &WithPayload<XRPLMessage>,
    ) -> Result<Vec<Event>, IngestorError> {
        let token_id = self
            .get_token_id(&xrpl_message_with_payload.message)
            .await?;

        let gas_credit_event = self
            .gas_credit_event_from_payment(xrpl_message_with_payload, token_id)
            .await?;

        self.verify_message(&xrpl_message_with_payload.message)
            .await?;

        Ok(vec![gas_credit_event])
    }

    pub async fn handle_add_reserves_message(
        &self,
        xrpl_message_with_payload: &WithPayload<XRPLMessage>,
    ) -> Result<Vec<Event>, IngestorError> {
        self.verify_message(&xrpl_message_with_payload.message)
            .await?;

        Ok(vec![])
    }

    pub async fn handle_prover_tx(&self, tx: Transaction) -> Result<Vec<Event>, IngestorError> {
        let unsigned_tx_hash = extract_and_decode_memo(&tx.common().memos, "unsigned_tx_hash")
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;
        let unsigned_tx_hash_bytes = hex::decode(&unsigned_tx_hash).map_err(|e| {
            IngestorError::GenericError(format!("Failed to decode unsigned tx hash: {}", e))
        })?;

        self.verify_message(&XRPLMessage::ProverMessage(XRPLProverMessage {
            tx_id: HexTxHash::new(
                std::convert::TryInto::<[u8; 32]>::try_into(
                    hex::decode(tx.common().hash.clone().ok_or_else(|| {
                        IngestorError::GenericError("Transaction missing hash".into())
                    })?)
                    .map_err(|_| IngestorError::GenericError("Cannot decode tx hash".into()))?,
                )
                .map_err(|_| {
                    IngestorError::GenericError(
                        "Cannot convert tx hash to bytes: invalid length".into(),
                    )
                })?,
            ),
            unsigned_tx_hash: HexTxHash::new(
                std::convert::TryInto::<[u8; 32]>::try_into(unsigned_tx_hash_bytes).map_err(
                    |_| {
                        IngestorError::GenericError(
                            "Cannot convert unsigned tx hash to bytes: invalid length".into(),
                        )
                    },
                )?,
            ),
        }))
        .await?;

        Ok(vec![])
    }

    async fn get_token_id(
        &self,
        xrpl_message: &XRPLMessage,
    ) -> Result<Option<TokenId>, IngestorError> {
        let token = match xrpl_message {
            XRPLMessage::InterchainTransferMessage(message) => message.transfer_amount.clone(),
            XRPLMessage::CallContractMessage(message) => message.gas_fee_amount.clone(),
            XRPLMessage::AddGasMessage(message) => message.amount.clone(),
            _ => {
                return Err(IngestorError::GenericError(
                    "Unsupported payment type".to_owned(),
                ))
            }
        };

        match token {
            XRPLPaymentAmount::Issued(token, _) => {
                let query = xrpl_gateway::msg::QueryMsg::XrplTokenId(token);

                let request = QueryRequest::Generic(serde_json::to_value(&query).map_err(|e| {
                    IngestorError::GenericError(format!("Failed to serialize query: {}", e))
                })?);

                let response_body = self
                    .gmp_api
                    .post_query(
                        self.config
                            .common_config
                            .axelar_contracts
                            .chain_gateway
                            .clone(),
                        &request,
                    )
                    .await
                    .map_err(|e| {
                        IngestorError::GenericError(format!("Failed to get token id: {}", e))
                    })?;

                let token_id: TokenId = serde_json::from_str(&response_body).map_err(|e| {
                    IngestorError::GenericError(format!("Failed to parse token id: {}", e))
                })?;

                Ok(Some(token_id))
            }
            XRPLPaymentAmount::Drops(_) => Ok(None),
        }
    }

    async fn translate_message(
        &self,
        xrpl_message: &XRPLMessage,
        payload: &Option<nonempty::HexBinary>,
    ) -> Result<(Option<MessageWithPayload>, TokenId), ITSTranslationError> {
        let query = match xrpl_message {
            XRPLMessage::InterchainTransferMessage(message) => {
                xrpl_gateway::msg::QueryMsg::InterchainTransfer {
                    message: message.clone(),
                    payload: payload.clone(),
                }
            }
            XRPLMessage::CallContractMessage(message) => {
                xrpl_gateway::msg::QueryMsg::CallContract {
                    message: message.clone(),
                    payload: payload.clone().ok_or(ITSTranslationError::NoPayload(
                        "Payload is required for CallContractMessage".to_owned(),
                    ))?,
                }
            }
            _ => {
                return Err(ITSTranslationError::InvalidMessageType(format!(
                    "{:?}",
                    xrpl_message
                )))
            }
        };

        let request = QueryRequest::Generic(serde_json::to_value(&query).map_err(|e| {
            ITSTranslationError::SerializationError(format!("Failed to serialize query: {}", e))
        })?);

        debug!("Sending query: {:?}", request);

        let response_body = self
            .gmp_api
            .post_query(
                self.config
                    .common_config
                    .axelar_contracts
                    .chain_gateway
                    .clone(),
                &request,
            )
            .await
            .map_err(|e| ITSTranslationError::RequestError(e.to_string()))?;

        match xrpl_message {
            XRPLMessage::InterchainTransferMessage(_) => {
                let interchain_transfer_response: InterchainTransfer =
                    serde_json::from_str(&response_body).map_err(|e| {
                        ITSTranslationError::SerializationError(format!(
                            "Failed to parse InterchainTransfer Message: {}",
                            e
                        ))
                    })?;
                Ok((
                    interchain_transfer_response.message_with_payload,
                    interchain_transfer_response.token_id,
                ))
            }
            XRPLMessage::CallContractMessage(_) => {
                let contract_call_response: CallContract = serde_json::from_str(&response_body)
                    .map_err(|e| {
                        ITSTranslationError::SerializationError(format!(
                            "Failed to parse CallContract Message: {}",
                            e
                        ))
                    })?;
                Ok((
                    Some(contract_call_response.message_with_payload),
                    contract_call_response.gas_token_id,
                ))
            }
            _ => Err(ITSTranslationError::InvalidMessageType(format!(
                "{:?}",
                xrpl_message
            ))),
        }
    }

    async fn call_event_from_message(
        &self,
        xrpl_message_with_payload: &WithPayload<XRPLMessage>,
    ) -> Result<Option<Event>, IngestorError> {
        let xrpl_message = xrpl_message_with_payload.message.clone();

        let source_context = HashMap::from([(
            "xrpl_message".to_owned(),
            serde_json::to_string(&xrpl_message).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize xrpl_message: {}", e))
            })?,
        )]);

        let translation_result = self
            .translate_message(&xrpl_message, &xrpl_message_with_payload.payload)
            .await;

        let message_with_payload = match translation_result {
            Err(
                ITSTranslationError::RequestError(e) | ITSTranslationError::SerializationError(e),
            ) => {
                return Err(IngestorError::GenericError(e));
            }
            Err(ITSTranslationError::InvalidMessageType(e) | ITSTranslationError::NoPayload(e)) => {
                warn!("ITS Translation: {}", e);
                return Ok(None);
            }
            Ok((message_with_payload, _)) => message_with_payload,
        };

        let message_with_payload = match message_with_payload {
            Some(message) => message,
            None => {
                warn!("ITS Translation: message_with_payload is None");
                return Ok(None);
            }
        };

        let xrpl_tx_hash = message_with_payload
            .message
            .cc_id
            .message_id
            .strip_prefix("0x")
            .ok_or(IngestorError::GenericError(
                "Message id is not prefixed with 0x".to_string(),
            ))?;

        self.models
            .xrpl_transaction_model
            .update_status(xrpl_tx_hash, XrplTransactionStatus::Initialized)
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let b64_payload = BASE64_STANDARD.encode(
            hex::decode(message_with_payload.payload.to_string()).map_err(|e| {
                IngestorError::GenericError(format!("Failed to decode payload: {}", e))
            })?,
        );

        Ok(Some(Event::Call {
            common: CommonEventFields {
                r#type: "CALL".to_owned(),
                event_id: format!("{}-call", xrpl_message.tx_id().to_string().to_lowercase()),
                meta: Some(EventMetadata {
                    tx_id: Some(xrpl_message.tx_id().to_string().to_lowercase()),
                    from_address: None,
                    finalized: None,
                    source_context: Some(source_context),
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message: GatewayV2Message {
                message_id: message_with_payload.message.cc_id.message_id.to_lowercase(),
                source_chain: message_with_payload.message.cc_id.source_chain.to_string(),
                source_address: message_with_payload.message.source_address.to_string(),
                destination_address: message_with_payload.message.destination_address.to_string(),
                payload_hash: hex::encode(message_with_payload.message.payload_hash),
            },
            destination_chain: message_with_payload.message.destination_chain.to_string(),
            payload: b64_payload,
        }))
    }

    pub async fn its_interchain_transfer_event(
        &self,
        xrpl_message: XRPLMessage,
        maybe_token_id: Option<TokenId>,
    ) -> Result<Event, IngestorError> {
        Ok(match xrpl_message {
            XRPLMessage::InterchainTransferMessage(message) => {
                let (transfer_amount, token_id) = match &message.transfer_amount {
                    XRPLPaymentAmount::Drops(amount) => {
                        let xrpl_token_id = self
                            .config
                            .common_config
                            .deployed_tokens
                            .iter()
                            .find(|(_, token_symbol)| token_symbol == &"XRP")
                            .ok_or(IngestorError::GenericError(
                                "XRP token id not found".to_string(),
                            ))?
                            .0;
                        (amount.to_string(), xrpl_token_id.to_owned())
                    }
                    XRPLPaymentAmount::Issued(_, amount) => {
                        if let Some(token_id) = maybe_token_id {
                            let amount =
                                Decimal::from_scientific(&amount.to_string()).map_err(|e| {
                                    IngestorError::GenericError(format!(
                                        "Failed to parse amount {}: {}",
                                        amount, e
                                    ))
                                })?;

                            let amount = convert_token_amount_to_drops(
                                &self.config.common_config,
                                amount,
                                &token_id.to_string(),
                                &self.price_view,
                            )
                            .await
                            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

                            (amount, token_id.to_string())
                        } else {
                            return Err(IngestorError::GenericError(
                                "Token id can't be None for IOU transfer".to_owned(),
                            ));
                        }
                    }
                };
                Event::ITSInterchainTransfer {
                    common: CommonEventFields {
                        r#type: "ITS/INTERCHAIN_TRANSFER".to_owned(),
                        event_id: format!("its-interchain-transfer-{}", message.tx_id),
                        meta: Some(EventMetadata {
                            tx_id: Some(message.tx_id.to_string().to_lowercase()),
                            from_address: None,
                            finalized: None,
                            source_context: None,
                            timestamp: chrono::Utc::now()
                                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        }),
                    },
                    message_id: message.tx_id.to_string(),
                    destination_chain: message.destination_chain.to_string(),
                    token_spent: Amount {
                        token_id: Some(token_id),
                        amount: transfer_amount,
                    },
                    source_address: message.source_address.to_string(),
                    destination_address: message.destination_address.to_string(),
                    data_hash: "0".repeat(32),
                }
            }
            _ => {
                return Err(IngestorError::GenericError(format!(
                    "Cannot create ITSInterchainTransfer event for message: {:?}",
                    xrpl_message
                )));
            }
        })
    }

    async fn gas_credit_event_from_payment(
        &self,
        xrpl_message_with_payload: &WithPayload<XRPLMessage>,
        maybe_token_id: Option<TokenId>,
    ) -> Result<Event, IngestorError> {
        let xrpl_message = xrpl_message_with_payload.message.clone();

        let tx_id = xrpl_message.tx_id().to_string().to_lowercase();
        let msg_id = match &xrpl_message {
            XRPLMessage::InterchainTransferMessage(_) => tx_id.clone(),
            XRPLMessage::CallContractMessage(_) => tx_id.clone(),
            XRPLMessage::AddGasMessage(message) => message.msg_id.to_string().to_lowercase(),
            _ => {
                return Err(IngestorError::GenericError(format!(
                    "Gas credit event not supported for this message type: {:?}",
                    xrpl_message
                )))
            }
        };

        let source_address = match &xrpl_message {
            XRPLMessage::InterchainTransferMessage(message) => message.source_address.to_string(),
            XRPLMessage::CallContractMessage(message) => message.source_address.to_string(),
            XRPLMessage::AddGasMessage(message) => message.source_address.to_string(),
            _ => {
                return Err(IngestorError::GenericError(
                    "Unsupported message type".to_owned(),
                ))
            }
        };

        let gas_fee_amount = match &xrpl_message {
            XRPLMessage::InterchainTransferMessage(message) => message.gas_fee_amount.clone(),
            XRPLMessage::CallContractMessage(message) => message.gas_fee_amount.clone(),
            XRPLMessage::AddGasMessage(message) => message.amount.clone(),
            _ => {
                return Err(IngestorError::GenericError(
                    "Unsupported message type".to_owned(),
                ))
            }
        };

        let gas_fee_amount_drops = match &gas_fee_amount {
            XRPLPaymentAmount::Drops(amount) => amount.to_string(),
            XRPLPaymentAmount::Issued(_, amount) => {
                if let Some(gas_token_id) = maybe_token_id {
                    let amount = Decimal::from_scientific(&amount.to_string()).map_err(|e| {
                        IngestorError::GenericError(format!(
                            "Failed to parse amount {}: {}",
                            amount, e
                        ))
                    })?;
                    debug!("Token transfer: {:?}", msg_id);
                    convert_token_amount_to_drops(
                        &self.config.common_config,
                        amount,
                        &gas_token_id.to_string(),
                        &self.price_view,
                    )
                    .await
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?
                } else {
                    return Err(IngestorError::GenericError(
                        "Gas token id can't be None for IOU transfer".to_owned(),
                    ));
                }
            }
        };

        Ok(Event::GasCredit {
            common: CommonEventFields {
                r#type: "GAS_CREDIT".to_owned(),
                event_id: format!("{}-gas", tx_id),
                meta: Some(EventMetadata {
                    tx_id: Some(xrpl_message.tx_id().to_string().to_lowercase()),
                    from_address: None,
                    finalized: None,
                    source_context: None,
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message_id: msg_id,
            refund_address: source_address,
            payment: gmp_types::Amount {
                token_id: None,
                amount: gas_fee_amount_drops,
            },
        })
    }

    pub async fn confirm_add_gas_message_request(
        &self,
        message: &XRPLAddGasMessage,
    ) -> Result<(String, BroadcastRequest), IngestorError> {
        let execute_msg =
            xrpl_gateway::msg::ExecuteMsg::ConfirmAddGasMessages(vec![message.clone()]);
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to serialize ConfirmAddGasMessages: {}",
                    e
                ))
            })?);
        Ok((
            self.config
                .common_config
                .axelar_contracts
                .chain_gateway
                .clone(),
            request,
        ))
    }

    pub async fn confirm_add_reserves_message_request(
        &self,
        message: &XRPLAddReservesMessage,
    ) -> Result<(String, BroadcastRequest), IngestorError> {
        let execute_msg = xrpl_multisig_prover::msg::ExecuteMsg::ConfirmAddReservesMessage {
            add_reserves_message: message.clone(),
        };
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to serialize ConfirmAddReservesMessage: {}",
                    e
                ))
            })?);
        Ok((
            self.config
                .common_config
                .axelar_contracts
                .chain_multisig_prover
                .clone(),
            request,
        ))
    }

    pub async fn confirm_prover_message_request(
        &self,
        tx: &Transaction,
    ) -> Result<(String, BroadcastRequest), IngestorError> {
        let tx_common = tx.common();

        let tx_hash = tx_common.hash.clone().ok_or_else(|| {
            IngestorError::GenericError("Transaction missing field 'hash'".into())
        })?;
        let tx_hash_bytes = hex::decode(&tx_hash)
            .map_err(|e| IngestorError::GenericError(format!("Failed to decode tx hash: {}", e)))?;

        let unsigned_tx_hash = extract_and_decode_memo(&tx_common.memos, "unsigned_tx_hash")
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;
        let unsigned_tx_hash_bytes = hex::decode(&unsigned_tx_hash).map_err(|e| {
            IngestorError::GenericError(format!("Failed to decode unsigned tx hash: {}", e))
        })?;

        let execute_msg = xrpl_multisig_prover::msg::ExecuteMsg::ConfirmProverMessage {
            prover_message: XRPLProverMessage {
                tx_id: HexTxHash::new(
                    std::convert::TryInto::<[u8; 32]>::try_into(tx_hash_bytes).map_err(|_| {
                        IngestorError::GenericError("Invalid length of tx hash bytes".into())
                    })?,
                ),
                unsigned_tx_hash: HexTxHash::new(
                    std::convert::TryInto::<[u8; 32]>::try_into(unsigned_tx_hash_bytes).map_err(
                        |_| {
                            IngestorError::GenericError(
                                "Invalid length of unsigned tx hash bytes".into(),
                            )
                        },
                    )?,
                ),
            },
        };
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize ConfirmTxStatus: {}", e))
            })?);
        Ok((
            self.config
                .common_config
                .axelar_contracts
                .chain_multisig_prover
                .clone(),
            request,
        ))
    }

    pub async fn route_incoming_message_request(
        &self,
        xrpl_message: &XRPLMessage,
    ) -> Result<(String, BroadcastRequest), IngestorError> {
        let payload_hash =
            match &xrpl_message {
                XRPLMessage::InterchainTransferMessage(msg) => msg.payload_hash,
                XRPLMessage::CallContractMessage(msg) => Some(msg.payload_hash),
                _ => return Err(IngestorError::GenericError(
                    "Routing only supported for InterchainTransferMessage and CallContractMessage"
                        .to_owned(),
                )),
            };

        let mut payload = None;
        if payload_hash.is_some() {
            let payload_string = self
                .gmp_api
                .get_payload(&hex::encode(payload_hash.unwrap()))
                .await
                .map_err(|e| {
                    IngestorError::GenericError(format!("Failed to get payload from cache: {}", e))
                })?;
            let payload_bytes = hex::decode(payload_string).map_err(|e| {
                IngestorError::GenericError(format!("Failed to decode payload: {}", e))
            })?;
            payload = Some(payload_bytes.try_into().map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to convert payload bytes to array: {}",
                    e
                ))
            })?);
        }
        let execute_msg = xrpl_gateway::msg::ExecuteMsg::RouteIncomingMessages(vec![WithPayload {
            message: xrpl_message.clone(),
            payload,
        }]);
        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to serialize RouteIncomingMessages: {}",
                    e
                ))
            })?);
        Ok((
            self.config
                .common_config
                .axelar_contracts
                .chain_gateway
                .clone(),
            request,
        ))
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

                        let maybe_tx_result = common.meta.map(|meta| meta.transaction_result);

                        // TODO: Don't send if the tx failed
                        // TODO: MessageExecuted could be moved earlier, right after broadcasting the message
                        let event = Event::MessageExecuted {
                            common: CommonEventFields {
                                r#type: "MESSAGE_EXECUTED".to_owned(),
                                event_id: common.hash.clone().ok_or(
                                    IngestorError::GenericError(
                                        "Transaction missing field 'hash'".into(),
                                    ),
                                )?,
                                meta: Some(MessageExecutedEventMetadata {
                                    common_meta: EventMetadata {
                                        tx_id: common.hash,
                                        from_address: None,
                                        finalized: None,
                                        source_context: None,
                                        timestamp: chrono::Utc::now()
                                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                                    },
                                    command_id: None,
                                    child_message_ids: None,
                                    revert_reason: if status == MessageExecutionStatus::REVERTED {
                                        maybe_tx_result.map(|tx_result| {
                                            serde_json::to_string(&tx_result).unwrap_or_default()
                                        })
                                    } else {
                                        None
                                    },
                                }),
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
                                    e
                                ))
                            })?;
                        let response =
                            events_response.first().ok_or(IngestorError::GenericError(
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

    async fn parse_payload_and_payload_hash(
        &self,
        memos: &Option<Vec<Memo>>,
    ) -> Result<(Option<String>, Option<String>), IngestorError> {
        let payload_hash_memo = extract_from_xrpl_memo(memos.clone(), "payload_hash");
        let payload_memo = extract_from_xrpl_memo(memos.clone(), "payload");

        // Payment transaction must not contain both 'payload' and 'payload_hash' memos.
        if payload_memo.is_ok() && payload_hash_memo.is_ok() {
            return Err(IngestorError::GenericError(
                "Payment transaction cannot have both 'payload' and 'payload_hash' memos"
                    .to_owned(),
            ));
        }

        if let Ok(payload_str) = payload_memo {
            // If we have 'payload', store it in the cache and receive the payload_hash.
            let hash = self
                .gmp_api
                .post_payload(&hex::decode(payload_str.clone()).unwrap())
                .await
                .map_err(|e| {
                    IngestorError::GenericError(format!("Failed to store payload in cache: {}", e))
                })?;
            Ok((Some(payload_str), Some(hash)))
        } else if let Ok(payload_hash_str) = payload_hash_memo {
            // If we have 'payload_hash', retrieve payload from the cache.
            let payload_retrieved =
                self.gmp_api
                    .get_payload(&payload_hash_str)
                    .await
                    .map_err(|e| {
                        IngestorError::GenericError(format!(
                            "Failed to get payload from cache: {}",
                            e
                        ))
                    })?;
            Ok((Some(payload_retrieved), Some(payload_hash_str)))
        } else {
            Ok((None, None))
        }
    }

    async fn build_xrpl_message(
        &self,
        payment: &PaymentTransaction,
    ) -> Result<WithPayload<XRPLMessage>, IngestorError> {
        let tx_hash = payment.common.hash.clone().ok_or_else(|| {
            IngestorError::GenericError("Payment transaction missing field 'hash'".to_owned())
        })?;
        let tx_id =
            HexTxHash::new(
                std::convert::TryInto::<[u8; 32]>::try_into(hex::decode(&tx_hash).map_err(
                    |_| IngestorError::GenericError("Failed to hex-decode tx_id".into()),
                )?)
                .map_err(|_| IngestorError::GenericError("Invalid length of tx_id bytes".into()))?,
            );
        let memos = &payment.common.memos;
        let message_type_str = extract_and_decode_memo(memos, "type")
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;
        let message_type: XRPLMessageType =
            serde_json::from_str(&format!("\"{}\"", message_type_str)).map_err(|e| {
                IngestorError::GenericError(format!(
                    "Failed to parse message type {}: {}",
                    message_type_str, e
                ))
            })?;
        let amount = parse_payment_amount(payment)?;
        let source_address: XRPLAccountId =
            payment.common.account.clone().try_into().map_err(|e| {
                IngestorError::GenericError(format!("Invalid source account: {:?}", e))
            })?;
        match message_type {
            XRPLMessageType::InterchainTransfer | XRPLMessageType::CallContract => {
                let gas_fee_amount = match message_type {
                    XRPLMessageType::InterchainTransfer => parse_gas_fee_amount(
                        &amount,
                        extract_and_decode_memo(memos, "gas_fee_amount")
                            .map_err(|e| IngestorError::GenericError(e.to_string()))?,
                    )?,
                    XRPLMessageType::CallContract => amount.clone(),
                    _ => unreachable!(),
                };

                let destination_chain = extract_and_decode_memo(memos, "destination_chain")
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?;
                let destination_address = extract_and_decode_memo(memos, "destination_address")
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?;

                let (payload, payload_hash) = self.parse_payload_and_payload_hash(memos).await?;

                let message = match message_type {
                    XRPLMessageType::InterchainTransfer => {
                        XRPLMessage::InterchainTransferMessage(XRPLInterchainTransferMessage {
                            tx_id,
                            source_address,
                            destination_address: destination_address.try_into().map_err(|e| {
                                IngestorError::GenericError(format!(
                                    "Invalid destination_address: {}",
                                    e
                                ))
                            })?,
                            destination_chain: ChainNameRaw::from_str(&destination_chain).map_err(
                                |e| {
                                    IngestorError::GenericError(format!(
                                        "Invalid destination_chain: {}",
                                        e
                                    ))
                                },
                            )?,
                            payload_hash: if payload_hash.is_some() {
                                Some(
                                    std::convert::TryInto::<[u8; 32]>::try_into(
                                        hex::decode(payload_hash.unwrap()).map_err(|_| {
                                            IngestorError::GenericError(
                                                "Failed to hex-decode payload_hash".into(),
                                            )
                                        })?,
                                    )
                                    .map_err(|_| {
                                        IngestorError::GenericError(
                                            "Invalid length of payload_hash bytes".into(),
                                        )
                                    })?,
                                )
                            } else {
                                None
                            },
                            transfer_amount: amount.sub(gas_fee_amount.clone()).map_err(
                                |e| match e {
                                    XRPLError::SubtractionUnderflow => IngestorError::GenericError(
                                        "Transfer amount is less than gas fee amount".to_owned(),
                                    ),
                                    _ => IngestorError::GenericError(format!(
                                        "Failed to subtract gas fee amount: {}",
                                        e
                                    )),
                                },
                            )?,
                            gas_fee_amount,
                        })
                    }
                    XRPLMessageType::CallContract => {
                        XRPLMessage::CallContractMessage(XRPLCallContractMessage {
                            tx_id,
                            source_address,
                            destination_address: destination_address.try_into().map_err(|e| {
                                IngestorError::GenericError(format!(
                                    "Invalid destination_address: {}",
                                    e
                                ))
                            })?,
                            destination_chain: ChainNameRaw::from_str(&destination_chain).map_err(
                                |e| {
                                    IngestorError::GenericError(format!(
                                        "Invalid destination_chain: {}",
                                        e
                                    ))
                                },
                            )?,
                            payload_hash: std::convert::TryInto::<[u8; 32]>::try_into(
                                hex::decode(payload_hash.ok_or(IngestorError::GenericError(
                                    "Payload hash is required for CallContract".into(),
                                ))?)
                                .map_err(|_| {
                                    IngestorError::GenericError(
                                        "Failed to hex-decode payload_hash".into(),
                                    )
                                })?,
                            )
                            .map_err(|_| {
                                IngestorError::GenericError(
                                    "Invalid length of payload_hash bytes".into(),
                                )
                            })?,
                            gas_fee_amount,
                        })
                    }
                    _ => unreachable!(),
                };

                let mut message_with_payload = WithPayload::new(message, None);

                if payload.is_some() {
                    let payload_bytes = hex::decode(payload.unwrap()).unwrap();
                    message_with_payload.payload = Some(payload_bytes.try_into().unwrap());
                }
                Ok(message_with_payload)
            }
            XRPLMessageType::AddGas => {
                let msg_tx_id = extract_and_decode_memo(memos, "msg_id")
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?;
                let msg_tx_id_bytes = hex::decode(&msg_tx_id).map_err(|e| {
                    IngestorError::GenericError(format!("Failed to decode unsigned tx hash: {}", e))
                })?;

                Ok(WithPayload::new(
                    XRPLMessage::AddGasMessage(XRPLAddGasMessage {
                        tx_id,
                        source_address,
                        msg_id: HexTxHash::new(
                            std::convert::TryInto::<[u8; 32]>::try_into(msg_tx_id_bytes).map_err(
                                |_| {
                                    IngestorError::GenericError(
                                        "Invalid length of msg_tx_id bytes".into(),
                                    )
                                },
                            )?,
                        ),
                        amount,
                    }),
                    None,
                ))
            }
            XRPLMessageType::AddReserves => match amount {
                XRPLPaymentAmount::Drops(amount) => Ok(WithPayload::new(
                    XRPLMessage::AddReservesMessage(XRPLAddReservesMessage { tx_id, amount }),
                    None,
                )),
                _ => Err(IngestorError::GenericError(
                    "Add fee reserve requires amount to be in XRP drops".to_owned(),
                )),
            },
            _ => Err(IngestorError::GenericError(format!(
                "Unsupported message type: {}",
                message_type
            ))),
        }
    }
}

impl<DB: Database, G: GmpApiTrait + Send + Sync + 'static> IngestorTrait for XrplIngestor<DB, G> {
    async fn handle_transaction(&self, tx: ChainTransaction) -> Result<Vec<Event>, IngestorError> {
        let ChainTransaction::Xrpl(tx) = tx else {
            return Err(IngestorError::UnexpectedChainTransactionType(format!("{:?}", tx)))
        };

        let xrpl_transaction =
            XrplTransaction::from_native_transaction(&tx, &self.config.xrpl_multisig)
                .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        match *tx.clone() {
            Transaction::Payment(_)
            | Transaction::TicketCreate(_)
            | Transaction::SignerListSet(_)
            | Transaction::TrustSet(_) => {
                if self
                    .models
                    .xrpl_transaction_model
                    .find(xrpl_transaction.tx_hash.clone())
                    .await
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?
                    .is_none()
                {
                    self.models
                        .xrpl_transaction_model
                        .upsert(xrpl_transaction.clone())
                        .await
                        .map_err(|e| IngestorError::GenericError(e.to_string()))?;
                }
            }
            _ => {}
        }

        match *tx.clone() {
            Transaction::Payment(payment) => {
                if payment.destination == self.config.xrpl_multisig {
                    if payment.common.memos.is_none() {
                        debug!("Skipping payment without memos: {:?}", payment);
                        return Ok(vec![]);
                    }

                    self.handle_payment(payment).await
                } else if payment.common.account == self.config.xrpl_multisig {
                    // prover message
                    self.handle_prover_tx(*tx).await
                } else {
                    Err(IngestorError::UnsupportedTransaction(
                        serde_json::to_string(&payment).map_err(|e| {
                            IngestorError::GenericError(format!(
                                "Failed to serialize payment: {}",
                                e
                            ))
                        })?,
                    ))
                }
            }
            Transaction::TicketCreate(_) => self.handle_prover_tx(*tx).await,
            Transaction::TrustSet(trust_set) => {
                if trust_set.common.account == self.config.xrpl_multisig {
                    return self.handle_prover_tx(*tx).await;
                }
                Ok(vec![])
            }
            Transaction::SignerListSet(_) => self.handle_prover_tx(*tx).await,
            tx => {
                warn!(
                    "Unsupported transaction type: {}",
                    serde_json::to_string(&tx).map_err(|e| {
                        IngestorError::GenericError(format!(
                            "Failed to serialize transaction: {}",
                            e
                        ))
                    })?
                );
                Ok(vec![])
            }
        }
    }

    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        let xrpl_message = parse_message_from_context(&task.common.meta)?;

        let xrpl_tx_hash = xrpl_message.tx_id().tx_hash_as_hex_no_prefix();
        self.models
            .xrpl_transaction_model
            .update_verify_task(
                &xrpl_tx_hash,
                &serde_json::to_string(&task).map_err(|e| {
                    IngestorError::GenericError(format!("Failed to serialize VerifyTask: {}", e))
                })?,
            )
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        self.verify_message(&xrpl_message).await?;

        Ok(())
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        let event_name = task.task.event.r#type.clone();

        // TODO: check the source contract of the event
        match event_name.as_str() {
            "wasm-quorum_reached" => {
                let content = event_attribute(&task.task.event, "content").ok_or_else(|| {
                    IngestorError::GenericError("QuorumReached event missing content".to_owned())
                })?;

                let tx_status: VerificationStatus = serde_json::from_str(
                    event_attribute(&task.task.event, "status")
                        .ok_or_else(|| {
                            IngestorError::GenericError(
                                "QuorumReached event for ProverMessage missing status".to_owned(),
                            )
                        })?
                        .as_str(),
                )
                .map_err(|e| {
                    IngestorError::GenericError(format!("Failed to parse status: {}", e))
                })?;

                let xrpl_message = match serde_json::from_str::<WithCrossChainId<XRPLMessage>>(&content) {
                    Ok(WithCrossChainId { content: message, cc_id: _ }) => message,
                    Err(_) => serde_json::from_str::<XRPLMessage>(&content).map_err(|e| {
                        IngestorError::GenericError(format!("Failed to parse content as either WithCrossChainId<XRPLMessage> or XRPLMessage: {}", e))
                    })?,
                };

                let xrpl_tx_hash = xrpl_message.tx_id().tx_hash_as_hex_no_prefix();
                self.models
                    .xrpl_transaction_model
                    .update_quorum_reached_task(
                        &xrpl_tx_hash,
                        &serde_json::to_string(&task).map_err(|e| {
                            IngestorError::GenericError(format!(
                                "Failed to serialize QuorumReachedTask: {}",
                                e
                            ))
                        })?,
                    )
                    .await
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?;

                match tx_status {
                    VerificationStatus::SucceededOnSourceChain => {}
                    VerificationStatus::FailedOnSourceChain => {}
                    _ => {
                        // TODO: should not skip
                        warn!("QuorumReached event has status: {}", tx_status);
                        return Ok(());
                    }
                };

                self.models
                    .xrpl_transaction_model
                    .update_status(&xrpl_tx_hash, XrplTransactionStatus::Verified)
                    .await
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?;

                let mut prover_tx = None;

                let (contract_address, request) = match &xrpl_message {
                    XRPLMessage::InterchainTransferMessage(_)
                    | XRPLMessage::CallContractMessage(_) => {
                        debug!("Quorum reached for XRPLMessage: {:?}", xrpl_message);
                        self.route_incoming_message_request(&xrpl_message).await?
                    }
                    XRPLMessage::ProverMessage(prover_message) => {
                        debug!(
                            "Quorum reached for XRPLProverMessage: {:?}",
                            prover_message.tx_id
                        );
                        let tx =
                            xrpl_tx_from_hash(prover_message.tx_id.clone(), &self.client).await?;
                        prover_tx = Some(tx);
                        self.confirm_prover_message_request(prover_tx.as_ref().unwrap())
                            .await?
                    }
                    XRPLMessage::AddGasMessage(msg) => {
                        self.confirm_add_gas_message_request(msg).await?
                    }
                    XRPLMessage::AddReservesMessage(msg) => {
                        self.confirm_add_reserves_message_request(msg).await?
                    }
                };

                debug!("Broadcasting request: {:?}", request);
                let maybe_confirmation_tx_hash = self
                    .gmp_api
                    .post_broadcast(contract_address, &request)
                    .await
                    .map_err(|e| {
                        IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
                    });

                match maybe_confirmation_tx_hash {
                    Ok(confirmation_tx_hash) => {
                        self.models
                            .xrpl_transaction_model
                            .update_route_tx(&xrpl_tx_hash, &confirmation_tx_hash.to_lowercase())
                            .await
                            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

                        self.models
                            .xrpl_transaction_model
                            .update_status(&xrpl_tx_hash, XrplTransactionStatus::Routed)
                            .await
                            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

                        info!(
                            "Confirm({}) tx hash: {}",
                            xrpl_message.tx_id(),
                            confirmation_tx_hash
                        );
                    }
                    Err(e) => {
                        if !e
                            .to_string()
                            .contains("transaction status is already confirmed")
                        {
                            return Err(e);
                        }
                        info!("Transaction {} is already confirmed", xrpl_message.tx_id());
                    }
                }

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

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        let cc_id = CrossChainId::new(
            task.task.message.source_chain.clone(),
            task.task.message.message_id.clone(),
        )
        .map_err(|e| {
            IngestorError::GenericError(format!("Failed to construct CrossChainId: {}", e))
        })?;

        let payload_bytes = BASE64_STANDARD.decode(&task.task.payload).map_err(|e| {
            IngestorError::GenericError(format!("Failed to decode task payload: {}", e))
        })?;

        let execute_msg = xrpl_multisig_prover::msg::ExecuteMsg::ConstructProof {
            cc_id: cc_id.clone(),
            payload: payload_bytes.clone().into(),
        };

        self.payload_cache
            .store(
                cc_id.clone(),
                PayloadCacheValue {
                    message: task.task.message.clone(),
                    payload: task.task.payload,
                },
            )
            .await
            .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        let request =
            BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
                IngestorError::GenericError(format!("Failed to serialize ConstructProof: {}", e))
            })?);

        let maybe_construct_proof_tx_hash = self
            .gmp_api
            .post_broadcast(
                self.config
                    .common_config
                    .axelar_contracts
                    .chain_multisig_prover
                    .clone(),
                &request,
            )
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
            });

        match maybe_construct_proof_tx_hash {
            Ok(tx_hash) => {
                info!("ConstructProof({}) transaction hash: {}", cc_id, tx_hash);
            }
            Err(e) => {
                let re = Regex::new(r"(payment for .*? already succeeded)").unwrap();

                if re.is_match(&e.to_string()) {
                    info!("Payment for {} already succeeded", cc_id);
                    return Ok(());
                }

                error!("Failed to construct proof: {}", e);
                self.gmp_api
                    .cannot_execute_message(
                        task.common.id,
                        task.task.message.message_id.clone(),
                        task.task.message.source_chain.clone(),
                        e.to_string(),
                        CannotExecuteMessageReason::Error
                    )
                    .await
                    .map_err(|e| IngestorError::GenericError(e.to_string()))?;
            }
        }

        Ok(())
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        let message_id = match message_id_from_retry_task(task.clone()).map_err(|e| {
            IngestorError::GenericError(format!("Failed to get message id from retry task: {}", e))
        })? {
            Some(message_id) => message_id,
            None => {
                debug!("Skipping retry task without message id: {:?}", task);
                return Ok(());
            }
        };

        let request_payload = task.request_payload();
        let invoked_contract_address = task.invoked_contract_address();

        let maybe_task_retries = self
            .models
            .task_retries
            .find(message_id.clone())
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to find task retries: {}", e))
            })?;

        let mut task_retries = if let Some(task_retries) = maybe_task_retries {
            task_retries
        } else {
            debug!("Creating task retries for message id: {}", message_id);
            TaskRetries {
                message_id: message_id.clone(),
                retries: 0,
                updated_at: chrono::Utc::now(),
            }
        };

        if task_retries.retries >= MAX_TASK_RETRIES {
            return Err(IngestorError::TaskMaxRetriesReached);
        }

        task_retries.retries += 1;

        info!("Retrying: {:?}", request_payload);

        let payload: BroadcastRequest = BroadcastRequest::Generic(
            serde_json::from_str(&request_payload)
                .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?,
        );

        let request = self
            .gmp_api
            .post_broadcast(invoked_contract_address, &payload)
            .await
            .map_err(|e| IngestorError::PostEventError(e.to_string()))?;

        info!("Broadcast request sent: {:?}", request);

        self.models
            .task_retries
            .upsert(task_retries)
            .await
            .map_err(|e| {
                IngestorError::GenericError(format!("Failed to increment task retries: {}", e))
            })?;

        Ok(())
    }
}
