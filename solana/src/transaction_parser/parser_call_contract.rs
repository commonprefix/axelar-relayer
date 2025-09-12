use crate::error::TransactionParsingError;
use crate::transaction_parser::discriminators::{CALL_CONTRACT_EVENT_DISC, CPI_EVENT_DISC};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::{Parser, ParserConfig};
use async_trait::async_trait;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use borsh::BorshDeserialize;
use relayer_base::gmp_api::gmp_types::{CommonEventFields, Event, EventMetadata, GatewayV2Message};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiCompiledInstruction;
use std::collections::HashMap;
use tracing::{debug, warn};

#[derive(BorshDeserialize, Clone, Debug)]
pub struct CallContractEvent {
    /// Sender's public key.
    pub sender_key: Pubkey,
    /// Payload hash, 32 bytes.
    pub payload_hash: [u8; 32],
    /// Destination chain as a `String`.
    pub destination_chain: String,
    /// Destination contract address as a `String`.
    pub destination_contract_address: String,
    /// Payload data as a `Vec<u8>`.
    pub payload: Vec<u8>,
}

pub struct ParserCallContract {
    signature: String,
    parsed: Option<CallContractEvent>,
    instruction: UiCompiledInstruction,
    config: ParserConfig,
    chain_name: String,
    index: u64,
}

impl ParserCallContract {
    pub(crate) async fn new(
        signature: String,
        instruction: UiCompiledInstruction,
        chain_name: String,
        index: u64,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            signature,
            parsed: None,
            instruction,
            config: ParserConfig {
                event_cpi_discriminator: CPI_EVENT_DISC,
                event_type_discriminator: CALL_CONTRACT_EVENT_DISC,
            },
            chain_name,
            index,
        })
    }

    fn try_extract_with_config(
        instruction: &UiCompiledInstruction,
        config: ParserConfig,
    ) -> Option<CallContractEvent> {
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
        match CallContractEvent::try_from_slice(payload) {
            Ok(event) => {
                debug!("Call Contract event={:?}", event);
                Some(event)
            }
            Err(_) => None,
        }
    }
}

#[async_trait]
impl Parser for ParserCallContract {
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
        let parsed = self
            .parsed
            .clone()
            .ok_or_else(|| TransactionParsingError::Message("Missing parsed".to_string()))?;

        Ok(MessageMatchingKey {
            destination_chain: parsed.destination_chain,
            destination_address: parsed.destination_contract_address,
            payload_hash: parsed.payload_hash,
        })
    }

    async fn event(&self, _message_id: Option<String>) -> Result<Event, TransactionParsingError> {
        let parsed = self
            .parsed
            .clone()
            .ok_or_else(|| TransactionParsingError::Message("Missing parsed".to_string()))?;

        let message_id = self
            .message_id()
            .await?
            .ok_or_else(|| TransactionParsingError::Message("Missing message_id".to_string()))?;

        let source_context = HashMap::from([
            ("source_address".to_owned(), parsed.sender_key.to_string()),
            (
                "destination_address".to_owned(),
                parsed.destination_contract_address.to_string(),
            ),
            (
                "destination_chain".to_owned(),
                parsed.destination_chain.clone(),
            ),
        ]);

        Ok(Event::Call {
            common: CommonEventFields {
                r#type: "CALL".to_owned(),
                event_id: format!("{}-call", self.signature.clone()),
                meta: Some(EventMetadata {
                    tx_id: Some(self.signature.clone()),
                    from_address: None,
                    finalized: None,
                    source_context: Some(source_context),
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message: GatewayV2Message {
                message_id,
                source_chain: self.chain_name.to_string(),
                source_address: parsed.sender_key.to_string(),
                destination_address: parsed.destination_contract_address.to_string(),
                payload_hash: BASE64_STANDARD.encode(parsed.payload_hash),
            },
            destination_chain: parsed.destination_chain.clone(),
            payload: BASE64_STANDARD.encode(parsed.payload),
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        Ok(Some(format!(
            "{}-{}",
            self.signature.to_string(),
            self.index
        )))
    }
}
