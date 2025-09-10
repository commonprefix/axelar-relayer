use crate::error::TransactionParsingError;
use crate::transaction_parser::discriminators::{CPI_EVENT_DISC, MESSAGE_EXECUTED_EVENT_DISC};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::{Parser, ParserConfig};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use bs58::encode;
use relayer_base::gmp_api::gmp_types::{
    Amount, CommonEventFields, Event, EventMetadata, MessageExecutedEventMetadata,
    MessageExecutionStatus,
};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiCompiledInstruction;
use tracing::{debug, warn};

#[derive(BorshDeserialize, Clone, Debug)]
pub struct MessageExecutedEvent {
    pub command_id: [u8; 32],
    pub destination_address: Pubkey,
    pub payload_hash: [u8; 32],
    pub source_chain: String,
    pub message_id: String,
    pub source_address: String,
    pub destination_chain: String,
}

pub struct ParserMessageExecuted {
    signature: String,
    parsed: Option<MessageExecutedEvent>,
    instruction: UiCompiledInstruction,
    config: ParserConfig,
}

impl ParserMessageExecuted {
    pub(crate) async fn new(
        signature: String,
        instruction: UiCompiledInstruction,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            signature,
            parsed: None,
            instruction,
            config: ParserConfig {
                event_cpi_discriminator: CPI_EVENT_DISC,
                event_type_discriminator: MESSAGE_EXECUTED_EVENT_DISC,
            },
        })
    }

    fn try_extract_with_config(
        instruction: &UiCompiledInstruction,
        config: ParserConfig,
    ) -> Option<MessageExecutedEvent> {
        let bytes = match bs58::decode(&instruction.data).into_vec() {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("failed to decode bytes: {:?}", e);
                return None;
            }
        };
        if bytes.len() < 16 {
            return None;
        }

        if &bytes[0..8] != &config.event_cpi_discriminator {
            warn!("expected event cpi discriminator, got {:?}", &bytes[0..8]);
            return None;
        }
        if &bytes[8..16] != &config.event_type_discriminator {
            warn!("expected event type discriminator, got {:?}", &bytes[8..16]);
            return None;
        }

        let payload = &bytes[16..];
        match MessageExecutedEvent::try_from_slice(payload) {
            Ok(event) => {
                debug!("Message Executed event={:?}", event);
                Some(event)
            }
            Err(e) => {
                warn!("failed to parse message executed event: {:?}", e);
                None
            }
        }
    }
}

#[async_trait]
impl Parser for ParserMessageExecuted {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.parsed.is_none() {
            self.parsed = Self::try_extract_with_config(&self.instruction, self.config);
        }
        Ok(self.parsed.is_some())
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        Ok(Self::try_extract_with_config(&self.instruction, self.config).is_some())
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        Err(TransactionParsingError::Message(
            "MessageMatchingKey is not available for MessageExecutedEvent".to_string(),
        ))
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let parsed = self
            .parsed
            .clone()
            .ok_or_else(|| TransactionParsingError::Message("Missing parsed".to_string()))?;

        Ok(Event::MessageExecuted {
            common: CommonEventFields {
                r#type: "MESSAGE_EXECUTED".to_owned(),
                event_id: self.signature.clone(),
                meta: Some(MessageExecutedEventMetadata {
                    common_meta: EventMetadata {
                        tx_id: Some(self.signature.clone()),
                        from_address: None,
                        finalized: None,
                        source_context: None,
                        timestamp: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    },
                    command_id: Some(encode(parsed.command_id).into_string()),
                    child_message_ids: None,
                    revert_reason: None,
                }),
            },
            message_id: parsed.message_id.clone(),
            source_chain: parsed.source_chain.clone(),
            status: MessageExecutionStatus::SUCCESSFUL,
            // figure out how to fill this?
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        Ok(None)
    }
}
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::test_utils::fixtures::fixture_traces;
//     use crate::transaction_parser::parser_message_executed::ParserMessageExecuted;

//     #[tokio::test]
//     async fn test_parser() {
//         let traces = fixture_traces();

//         let tx = traces[0].transactions[3].clone();
//         let address = tx.clone().account;

//         let mut parser = ParserMessageExecuted::new(tx, address).await.unwrap();
//         assert!(parser.is_match().await.unwrap());
//         parser.parse().await.unwrap();
//         let event = parser.event(None).await.unwrap();
//         match event {
//             Event::MessageExecuted {
//                 common,
//                 message_id,
//                 source_chain,
//                 status,
//                 cost,
//             } => {
//                 assert_eq!(
//                     message_id,
//                     "0xf2b741fb0b2c2fcf92aca82395bc65dab4dd8239a12f366d6045755e0b02c2a2-1"
//                 );
//                 assert_eq!(source_chain, "avalanche-fuji");
//                 assert_eq!(status, MessageExecutionStatus::SUCCESSFUL);
//                 assert_eq!(cost.amount, "0");
//                 assert_eq!(cost.token_id.as_deref(), None);

//                 let meta = &common.meta.as_ref().unwrap();
//                 assert_eq!(meta.common_meta.tx_id.as_deref(), Some("aa4"));
//                 assert_eq!(meta.revert_reason.as_deref(), None);
//             }
//             _ => panic!("Expected MessageExecuted event"),
//         }
//     }
// }
