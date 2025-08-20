use crate::client::SolanaClient;
use crate::config::SolanaConfig;
use crate::models::solana_transaction::{
    PgSolanaTransactionModel, SolanaTransaction, SolanaTransactionStatus,
};
use axelar_wasm_std::{msg_id::HexTxHash, nonempty};
use base64::prelude::*;
use interchain_token_service::TokenId;
use regex::Regex;
use relayer_base::gmp_api::gmp_types::{CannotExecuteMessageReason, RetryTask, VerificationStatus};
use relayer_base::gmp_api::GmpApiTrait;
use relayer_base::ingestor::IngestorTrait;
use relayer_base::models::task_retries::{PgTaskRetriesModel, TaskRetries};
use relayer_base::payload_cache::PayloadCacheTrait;
use relayer_base::subscriber::ChainTransaction;
use relayer_base::utils::{extract_from_xrpl_memo, ThreadSafe};
use relayer_base::{
    database::Database,
    error::{ITSTranslationError, IngestorError},
    gmp_api::gmp_types::{
        self, Amount, BroadcastRequest, CommonEventFields, ConstructProofTask, Event,
        EventMetadata, GatewayV2Message, MessageExecutedEventMetadata, MessageExecutionStatus,
        QueryRequest, ReactToWasmEventTask, VerifyTask,
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
use rust_decimal::Decimal;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::transaction::Transaction;
use std::{collections::HashMap, str::FromStr, sync::Arc, vec};
use tracing::{debug, error, info, warn};

const MAX_TASK_RETRIES: i32 = 5;

pub struct SolanaIngestorModels {
    pub solana_transaction_model: PgSolanaTransactionModel,
    pub task_retries: PgTaskRetriesModel,
}

pub struct SolanaIngestor<DB: Database, G: GmpApiTrait + ThreadSafe> {
    client: SolanaClient,
    gmp_api: Arc<G>,
    config: SolanaConfig,
    price_view: PriceView<DB>,
    payload_cache: PayloadCache<DB>,
    models: SolanaIngestorModels,
}

impl<DB, G> SolanaIngestor<DB, G>
where
    DB: Database,
    G: GmpApiTrait + ThreadSafe,
{
    pub fn new(
        gmp_api: Arc<G>,
        config: SolanaConfig,
        price_view: PriceView<DB>,
        payload_cache: PayloadCache<DB>,
        models: SolanaIngestorModels,
    ) -> Self {
        let client = SolanaClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3)
            .expect("failed to create Solana RPC client");
        Self {
            gmp_api,
            config,
            client,
            price_view,
            payload_cache,
            models,
        }
    }
}

impl<DB, G> IngestorTrait for SolanaIngestor<DB, G>
where
    DB: Database,
    G: GmpApiTrait + ThreadSafe,
{
    async fn handle_transaction(&self, tx: ChainTransaction) -> Result<Vec<Event>, IngestorError> {
        // let ChainTransaction::Solana(tx) = tx else {
        //     return Err(IngestorError::UnexpectedChainTransactionType(format!(
        //         "{tx:?}"
        //     )));
        // };

        // let solana_transaction =
        //     SolanaTransaction::from_native_transaction(&tx, &self.config.solana_multisig)
        //         .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        // match *tx.clone() {
        //     Transaction::Payment(_)
        //     | Transaction::TicketCreate(_)
        //     | Transaction::SignerListSet(_)
        //     | Transaction::TrustSet(_) => {
        //         if self
        //             .models
        //             .xrpl_transaction_model
        //             .find(xrpl_transaction.tx_hash.clone())
        //             .await
        //             .map_err(|e| IngestorError::GenericError(e.to_string()))?
        //             .is_none()
        //         {
        //             self.models
        //                 .xrpl_transaction_model
        //                 .upsert(xrpl_transaction.clone())
        //                 .await
        //                 .map_err(|e| IngestorError::GenericError(e.to_string()))?;
        //         }
        //     }
        //     _ => {}
        // }

        // match *tx.clone() {
        //     Transaction::Payment(payment) => {
        //         if payment.destination == self.config.xrpl_multisig {
        //             if payment.common.memos.is_none() {
        //                 debug!("Skipping payment without memos: {:?}", payment);
        //                 return Ok(vec![]);
        //             }

        //             self.handle_payment(payment).await
        //         } else if payment.common.account == self.config.xrpl_multisig {
        //             // prover message
        //             self.handle_prover_tx(*tx).await
        //         } else {
        //             info!(
        //                 "Skipping payment that is not for or from the multisig: {:?}",
        //                 payment
        //             );
        //             return Ok(vec![]);
        //         }
        //     }
        //     Transaction::TicketCreate(_) => self.handle_prover_tx(*tx).await,
        //     Transaction::TrustSet(trust_set) => {
        //         if trust_set.common.account == self.config.xrpl_multisig {
        //             return self.handle_prover_tx(*tx).await;
        //         }
        //         Ok(vec![])
        //     }
        //     Transaction::SignerListSet(_) => self.handle_prover_tx(*tx).await,
        //     tx => {
        //         info!(
        //             "Unsupported transaction type: {}",
        //             serde_json::to_string(&tx).map_err(|e| {
        //                 IngestorError::GenericError(format!(
        //                     "Failed to serialize transaction: {}",
        //                     e
        //                 ))
        //             })?
        //         );
        //         Ok(vec![])
        //     }
        // }
        Err(IngestorError::GenericError("Not implemented".to_string()))
    }

    async fn handle_verify(&self, task: VerifyTask) -> Result<(), IngestorError> {
        // let xrpl_message = parse_message_from_context(&task.common.meta)?;

        // let xrpl_tx_hash = xrpl_message.tx_id().tx_hash_as_hex_no_prefix();
        // self.models
        //     .xrpl_transaction_model
        //     .update_verify_task(
        //         &xrpl_tx_hash,
        //         &serde_json::to_string(&task).map_err(|e| {
        //             IngestorError::GenericError(format!("Failed to serialize VerifyTask: {}", e))
        //         })?,
        //     )
        //     .await
        //     .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        // self.verify_message(&xrpl_message).await?;

        // Ok(())
        Err(IngestorError::GenericError("Not implemented".to_string()))
    }

    async fn handle_wasm_event(&self, task: ReactToWasmEventTask) -> Result<(), IngestorError> {
        // let event_name = task.task.event.r#type.clone();

        // // TODO: check the source contract of the event
        // match event_name.as_str() {
        //     "wasm-quorum_reached" => {
        //         let content = event_attribute(&task.task.event, "content").ok_or_else(|| {
        //             IngestorError::GenericError("QuorumReached event missing content".to_owned())
        //         })?;

        //         let tx_status: VerificationStatus = serde_json::from_str(
        //             event_attribute(&task.task.event, "status")
        //                 .ok_or_else(|| {
        //                     IngestorError::GenericError(
        //                         "QuorumReached event for ProverMessage missing status".to_owned(),
        //                     )
        //                 })?
        //                 .as_str(),
        //         )
        //         .map_err(|e| {
        //             IngestorError::GenericError(format!("Failed to parse status: {}", e))
        //         })?;

        //         let xrpl_message = match serde_json::from_str::<WithCrossChainId<XRPLMessage>>(&content) {
        //             Ok(WithCrossChainId { content: message, cc_id: _ }) => message,
        //             Err(_) => serde_json::from_str::<XRPLMessage>(&content).map_err(|e| {
        //                 IngestorError::GenericError(format!("Failed to parse content as either WithCrossChainId<XRPLMessage> or XRPLMessage: {}", e))
        //             })?,
        //         };

        //         let xrpl_tx_hash = xrpl_message.tx_id().tx_hash_as_hex_no_prefix();
        //         self.models
        //             .xrpl_transaction_model
        //             .update_quorum_reached_task(
        //                 &xrpl_tx_hash,
        //                 &serde_json::to_string(&task).map_err(|e| {
        //                     IngestorError::GenericError(format!(
        //                         "Failed to serialize QuorumReachedTask: {}",
        //                         e
        //                     ))
        //                 })?,
        //             )
        //             .await
        //             .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        //         match tx_status {
        //             VerificationStatus::SucceededOnSourceChain => {}
        //             VerificationStatus::FailedOnSourceChain => {}
        //             _ => {
        //                 // TODO: should not skip
        //                 warn!("QuorumReached event has status: {}", tx_status);
        //                 return Ok(());
        //             }
        //         };

        //         self.models
        //             .xrpl_transaction_model
        //             .update_status(&xrpl_tx_hash, XrplTransactionStatus::Verified)
        //             .await
        //             .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        //         let mut prover_tx = None;

        //         let (contract_address, request) = match &xrpl_message {
        //             XRPLMessage::InterchainTransferMessage(_)
        //             | XRPLMessage::CallContractMessage(_) => {
        //                 debug!("Quorum reached for XRPLMessage: {:?}", xrpl_message);
        //                 self.route_incoming_message_request(&xrpl_message).await?
        //             }
        //             XRPLMessage::ProverMessage(prover_message) => {
        //                 debug!(
        //                     "Quorum reached for XRPLProverMessage: {:?}",
        //                     prover_message.tx_id
        //                 );
        //                 let tx =
        //                     xrpl_tx_from_hash(prover_message.tx_id.clone(), &self.client).await?;
        //                 prover_tx = Some(tx);
        //                 self.confirm_prover_message_request(prover_tx.as_ref().ok_or(
        //                     IngestorError::GenericError("Prover transaction is None".to_owned()),
        //                 )?)
        //                 .await?
        //             }
        //             XRPLMessage::AddGasMessage(msg) => {
        //                 self.confirm_add_gas_message_request(msg).await?
        //             }
        //             XRPLMessage::AddReservesMessage(msg) => {
        //                 self.confirm_add_reserves_message_request(msg).await?
        //             }
        //         };

        //         debug!("Broadcasting request: {:?}", request);
        //         let maybe_confirmation_tx_hash = self
        //             .gmp_api
        //             .post_broadcast(contract_address, &request)
        //             .await
        //             .map_err(|e| {
        //                 IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
        //             });

        //         match maybe_confirmation_tx_hash {
        //             Ok(confirmation_tx_hash) => {
        //                 self.models
        //                     .xrpl_transaction_model
        //                     .update_route_tx(&xrpl_tx_hash, &confirmation_tx_hash.to_lowercase())
        //                     .await
        //                     .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        //                 self.models
        //                     .xrpl_transaction_model
        //                     .update_status(&xrpl_tx_hash, XrplTransactionStatus::Routed)
        //                     .await
        //                     .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        //                 info!(
        //                     "Confirm({}) tx hash: {}",
        //                     xrpl_message.tx_id(),
        //                     confirmation_tx_hash
        //                 );
        //             }
        //             Err(e) => {
        //                 if !e
        //                     .to_string()
        //                     .contains("transaction status is already confirmed")
        //                 {
        //                     return Err(e);
        //                 }
        //                 info!("Transaction {} is already confirmed", xrpl_message.tx_id());
        //             }
        //         }

        //         self.handle_successful_routing(xrpl_message, prover_tx, task)
        //             .await?;

        //         Ok(())
        //     }
        //     _ => Err(IngestorError::GenericError(format!(
        //         "Unknown event name: {}",
        //         event_name
        //     ))),
        // }
        Err(IngestorError::GenericError("Not implemented".to_string()))
    }

    async fn handle_construct_proof(&self, task: ConstructProofTask) -> Result<(), IngestorError> {
        // let cc_id = CrossChainId::new(
        //     task.task.message.source_chain.clone(),
        //     task.task.message.message_id.clone(),
        // )
        // .map_err(|e| {
        //     IngestorError::GenericError(format!("Failed to construct CrossChainId: {}", e))
        // })?;

        // let payload_bytes = BASE64_STANDARD.decode(&task.task.payload).map_err(|e| {
        //     IngestorError::GenericError(format!("Failed to decode task payload: {}", e))
        // })?;

        // let execute_msg = xrpl_multisig_prover::msg::ExecuteMsg::ConstructProof {
        //     cc_id: cc_id.clone(),
        //     payload: payload_bytes.clone().into(),
        // };

        // self.payload_cache
        //     .store(
        //         cc_id.clone(),
        //         PayloadCacheValue {
        //             message: task.task.message.clone(),
        //             payload: task.task.payload,
        //         },
        //     )
        //     .await
        //     .map_err(|e| IngestorError::GenericError(e.to_string()))?;

        // let request =
        //     BroadcastRequest::Generic(serde_json::to_value(&execute_msg).map_err(|e| {
        //         IngestorError::GenericError(format!("Failed to serialize ConstructProof: {}", e))
        //     })?);

        // let maybe_construct_proof_tx_hash = self
        //     .gmp_api
        //     .post_broadcast(
        //         self.config
        //             .common_config
        //             .axelar_contracts
        //             .chain_multisig_prover
        //             .clone(),
        //         &request,
        //     )
        //     .await
        //     .map_err(|e| {
        //         IngestorError::GenericError(format!("Failed to broadcast message: {}", e))
        //     });

        // match maybe_construct_proof_tx_hash {
        //     Ok(tx_hash) => {
        //         info!("ConstructProof({}) transaction hash: {}", cc_id, tx_hash);
        //     }
        //     Err(e) => {
        //         let re = Regex::new(r"(payment for .*? already succeeded)").map_err(|e| {
        //             IngestorError::GenericError(format!("Failed to create regex: {}", e))
        //         })?;

        //         if re.is_match(&e.to_string()) {
        //             info!("Payment for {} already succeeded", cc_id);
        //             return Ok(());
        //         }

        //         error!("Failed to construct proof: {}", e);
        //         self.gmp_api
        //             .cannot_execute_message(
        //                 task.common.id,
        //                 task.task.message.message_id.clone(),
        //                 task.task.message.source_chain.clone(),
        //                 e.to_string(),
        //                 CannotExecuteMessageReason::Error,
        //             )
        //             .await
        //             .map_err(|e| IngestorError::GenericError(e.to_string()))?;
        //     }
        // }

        // Ok(())
        Err(IngestorError::GenericError("Not implemented".to_string()))
    }

    async fn handle_retriable_task(&self, task: RetryTask) -> Result<(), IngestorError> {
        // let message_id = match message_id_from_retry_task(task.clone()).map_err(|e| {
        //     IngestorError::GenericError(format!("Failed to get message id from retry task: {}", e))
        // })? {
        //     Some(message_id) => message_id,
        //     None => {
        //         debug!("Skipping retry task without message id: {:?}", task);
        //         return Ok(());
        //     }
        // };

        // let request_payload = task.request_payload();
        // let invoked_contract_address = task.invoked_contract_address();

        // let maybe_task_retries = self
        //     .models
        //     .task_retries
        //     .find(message_id.clone())
        //     .await
        //     .map_err(|e| {
        //         IngestorError::GenericError(format!("Failed to find task retries: {}", e))
        //     })?;

        // let mut task_retries = if let Some(task_retries) = maybe_task_retries {
        //     task_retries
        // } else {
        //     debug!("Creating task retries for message id: {}", message_id);
        //     TaskRetries {
        //         message_id: message_id.clone(),
        //         retries: 0,
        //         updated_at: chrono::Utc::now(),
        //     }
        // };

        // if task_retries.retries >= MAX_TASK_RETRIES {
        //     return Err(IngestorError::TaskMaxRetriesReached);
        // }

        // task_retries.retries += 1;

        // info!("Retrying: {:?}", request_payload);

        // let payload: BroadcastRequest = BroadcastRequest::Generic(
        //     serde_json::from_str(&request_payload)
        //         .map_err(|e| IngestorError::ParseError(format!("Invalid JSON: {}", e)))?,
        // );

        // let request = self
        //     .gmp_api
        //     .post_broadcast(invoked_contract_address, &payload)
        //     .await
        //     .map_err(|e| IngestorError::PostEventError(e.to_string()))?;

        // info!("Broadcast request sent: {:?}", request);

        // self.models
        //     .task_retries
        //     .upsert(task_retries)
        //     .await
        //     .map_err(|e| {
        //         IngestorError::GenericError(format!("Failed to increment task retries: {}", e))
        //     })?;

        // Ok(())
        Err(IngestorError::GenericError("Not implemented".to_string()))
    }
}
