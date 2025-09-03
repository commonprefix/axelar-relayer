// use crate::error::TransactionParsingError;
// use crate::transaction_parser::common::{hash_to_message_id, is_log_emmitted};
// use crate::transaction_parser::message_matching_key::MessageMatchingKey;
// use crate::transaction_parser::parser::Parser;
// use async_trait::async_trait;
// // use base64::prelude::BASE64_STANDARD;
// // use base64::Engine;
// use relayer_base::gmp_api::gmp_types::{CommonEventFields, Event, EventMetadata, GatewayV2Message};
// use solana_sdk::pubkey::Pubkey;
// use solana_types::solana_types::SolanaTransaction;
// use std::collections::HashMap;

// pub struct ParserCallContract {
//     log: Option<CallContractMessage>,
//     tx: SolanaTransaction,
//     allowed_address: Pubkey,
//     chain_name: String,
// }

// impl ParserCallContract {
//     pub(crate) async fn new(
//         tx: SolanaTransaction,
//         allowed_address: Pubkey,
//         chain_name: String,
//     ) -> Result<Self, TransactionParsingError> {
//         Ok(Self {
//             log: None,
//             tx,
//             allowed_address,
//             chain_name,
//         })
//     }
// }

// #[async_trait]
// impl Parser for ParserCallContract {
//     async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
//         if self.log.is_none() {
//             self.log = Some(
//                 CallContractMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body)
//                     .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
//             );
//         }
//         Ok(true)
//     }

//     async fn is_match(&self) -> Result<bool, TransactionParsingError> {
//         if self.tx.message.account_keys[0] != self.allowed_address {
//             return Ok(false);
//         }

//         is_log_emmitted(&self.tx, OP_CALL_CONTRACT, 0)
//     }

//     async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
//         let log = match self.log.clone() {
//             Some(log) => log,
//             None => return Err(TransactionParsingError::Message("Missing log".to_string())),
//         };
//         let key = MessageMatchingKey {
//             destination_chain: log.destination_chain.clone(),
//             destination_address: log.destination_address.clone(),
//             payload_hash: log.payload_hash,
//         };

//         Ok(key)
//     }

//     async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
//         let tx = &self.tx;
//         let log = match self.log.clone() {
//             Some(log) => log,
//             None => return Err(TransactionParsingError::Message("Missing log".to_string())),
//         };
//         let message_id = match self.message_id().await? {
//             Some(id) => id,
//             None => {
//                 return Err(TransactionParsingError::Message(
//                     "Missing message id".to_string(),
//                 ))
//             }
//         };

//         let source_context = HashMap::from([
//             ("source_address".to_owned(), log.source_address.to_hex()),
//             (
//                 "destination_address".to_owned(),
//                 log.destination_address.to_string(),
//             ),
//             (
//                 "destination_chain".to_owned(),
//                 log.destination_chain.clone(),
//             ),
//         ]);

//         let decoded = hex::decode(log.payload).map_err(|e| {
//             TransactionParsingError::BocParsing(format!("Failed to decode payload: {e}"))
//         })?;

//         let b64_payload = BASE64_STANDARD.encode(decoded);

//         Ok(Event::Call {
//             common: CommonEventFields {
//                 r#type: "CALL".to_owned(),
//                 event_id: tx.hash.clone(),
//                 meta: Some(EventMetadata {
//                     tx_id: Some(tx.hash.clone()),
//                     from_address: None,
//                     finalized: None,
//                     source_context: Some(source_context),
//                     timestamp: chrono::Utc::now()
//                         .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
//                 }),
//             },
//             message: GatewayV2Message {
//                 message_id,
//                 source_chain: self.chain_name.to_string(),
//                 source_address: log.source_address.to_hex(),
//                 destination_address: log.destination_address.to_string(),
//                 payload_hash: BASE64_STANDARD.encode(log.payload_hash),
//             },
//             destination_chain: log.destination_chain.clone(),
//             payload: b64_payload,
//         })
//     }

//     async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
//         Ok(Some(hash_to_message_id(&self.tx.hash).map_err(|e| {
//             TransactionParsingError::Message(e.to_string())
//         })?))
//     }
// }
