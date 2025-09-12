use crate::error::TransactionParsingError;
use crate::transaction_parser::discriminators::{
    CANNOT_EXECUTE_MESSAGE_EVENT_DISC, CPI_EVENT_DISC,
};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::{Parser, ParserConfig};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use relayer_base::gmp_api::gmp_types::{CannotExecuteMessageReason, CommonEventFields, Event};
use solana_transaction_status::UiCompiledInstruction;
use tracing::{debug, warn};

#[derive(BorshDeserialize, Clone, Debug)]
pub struct ExecuteInsufficientGasEvent {
    pub message_id: String,
    pub source_chain: String,
}

pub struct ParserExecuteInsufficientGas {
    signature: String,
    parsed: Option<ExecuteInsufficientGasEvent>,
    instruction: UiCompiledInstruction,
    config: ParserConfig,
}

impl ParserExecuteInsufficientGas {
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
                event_type_discriminator: CANNOT_EXECUTE_MESSAGE_EVENT_DISC,
            },
        })
    }

    fn try_extract_with_config(
        instruction: &UiCompiledInstruction,
        config: ParserConfig,
    ) -> Option<ExecuteInsufficientGasEvent> {
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

        if bytes.get(0..8) != Some(&config.event_cpi_discriminator) {
            debug!(
                "expected event cpi discriminator, got {:?}",
                bytes.get(0..8)
            );
            return None;
        }
        if bytes.get(8..16) != Some(&config.event_type_discriminator) {
            debug!(
                "expected event type discriminator, got {:?}",
                bytes.get(8..16)
            );
            return None;
        }

        let payload = bytes.get(16..)?;
        match ExecuteInsufficientGasEvent::try_from_slice(payload) {
            Ok(event) => {
                debug!("Message Executed event={:?}", event);
                Some(event)
            }
            Err(_) => None,
        }
    }
}

#[async_trait]
impl Parser for ParserExecuteInsufficientGas {
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
            "MessageMatchingKey is not available for ExecuteInsufficientGasEvent".to_string(),
        ))
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let parsed = self
            .parsed
            .clone()
            .ok_or_else(|| TransactionParsingError::Message("Missing parsed".to_string()))?;

        Ok(Event::CannotExecuteMessageV2 {
            common: CommonEventFields {
                r#type: "CANNOT_EXECUTE_MESSAGE/V2".to_owned(),
                event_id: format!("cannot-execute-task-v2-{}", self.signature),
                meta: None,
            },
            message_id: parsed.message_id,
            source_chain: parsed.source_chain,
            reason: CannotExecuteMessageReason::InsufficientGas,
            details: self.signature.to_string(),
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        Ok(None)
    }
}
