use crate::error::TransactionParsingError;
use crate::transaction_parser::discriminators::{CPI_EVENT_DISC, NATIVE_GAS_REFUNDED_EVENT_DISC};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::{Parser, ParserConfig};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiCompiledInstruction;
use tracing::{debug, warn};

#[derive(BorshDeserialize, Clone, Debug)]
pub struct NativeGasRefundedEvent {
    /// Solana transaction signature
    pub tx_hash: [u8; 64],
    /// The Gas service config PDA
    pub _config_pda: Pubkey,
    /// The log index
    pub log_index: u64,
    /// The receiver of the refund
    pub receiver: Pubkey,
    /// amount of SOL
    pub fees: u64,
}

pub struct ParserNativeGasRefunded {
    signature: String,
    parsed: Option<NativeGasRefundedEvent>,
    instruction: UiCompiledInstruction,
    config: ParserConfig,
}

impl ParserNativeGasRefunded {
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
                event_type_discriminator: NATIVE_GAS_REFUNDED_EVENT_DISC,
            },
        })
    }

    fn try_extract_with_config(
        instruction: &UiCompiledInstruction,
        config: ParserConfig,
    ) -> Option<NativeGasRefundedEvent> {
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
            warn!(
                "expected event cpi discriminator, got {:?}",
                bytes.get(0..8)
            );
            return None;
        }
        if bytes.get(8..16) != Some(&config.event_type_discriminator) {
            warn!(
                "expected event type discriminator, got {:?}",
                bytes.get(8..16)
            );
            return None;
        }

        let payload = bytes.get(16..)?;
        match NativeGasRefundedEvent::try_from_slice(payload) {
            Ok(event) => {
                debug!("Native Gas Refunded event={:?}", event);
                Some(event)
            }
            Err(e) => {
                warn!("failed to parse native gas refunded event: {:?}", e);
                None
            }
        }
    }
}

#[async_trait]
impl Parser for ParserNativeGasRefunded {
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
            "MessageMatchingKey is not available for NativeGasRefundedEvent".to_string(),
        ))
    }

    async fn event(&self, _message_id: Option<String>) -> Result<Event, TransactionParsingError> {
        let parsed = self
            .parsed
            .clone()
            .ok_or_else(|| TransactionParsingError::Message("Missing parsed".to_string()))?;

        let message_id = match self.message_id().await? {
            Some(id) => id,
            None => {
                return Err(TransactionParsingError::Message(
                    "Missing message id".to_string(),
                ))
            }
        };

        Ok(Event::GasRefunded {
            common: CommonEventFields {
                r#type: "GAS_REFUNDED".to_owned(),
                event_id: format!("{}-refund", self.signature.clone()),
                meta: None,
            },
            message_id,
            recipient_address: parsed.receiver.to_string(),
            refunded_amount: Amount {
                token_id: None,
                amount: parsed.fees.to_string(),
            },
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        if let Some(parsed) = self.parsed.clone() {
            Ok(Some(format!("{:?}-{}", parsed.tx_hash, parsed.log_index)))
        } else {
            Ok(None)
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use solana_transaction_status::UiCompiledInstruction;

//     #[tokio::test]
//     async fn test_decode_and_parse_gas_refunded_event() {
//         let mut data: Vec<u8> = Vec::new();
//         data.extend_from_slice(&CPI_EVENT_DISC);
//         data.extend_from_slice(&NATIVE_GAS_REFUNDED_EVENT_DISC);
//         let tx_hash_bytes = [0xABu8; 64];
//         data.extend_from_slice(&tx_hash_bytes);
//         data.extend_from_slice(&[0x11; 32]);
//         data.extend_from_slice(&(7u64).to_le_bytes());
//         data.extend_from_slice(&[0x33; 32]);
//         data.extend_from_slice(&(4242u64).to_le_bytes());

//         let ci = UiCompiledInstruction {
//             program_id_index: 4,
//             accounts: vec![3],
//             data: bs58::encode(data).into_string(),
//             stack_height: Some(2),
//         };

//         let mut parser = ParserNativeGasRefunded::new("dummy_sig".to_string(), ci)
//             .await
//             .unwrap();

//         assert!(parser.is_match().await.unwrap(), "parser should match");
//         assert!(parser.parse().await.unwrap(), "parser should parse message");

//         let mid = parser
//             .message_id()
//             .await
//             .expect("message_id ok")
//             .expect("message_id some");
//         let expected_mid = format!("0x{}", (0..64).map(|_| "ab").collect::<Vec<_>>().join(""));
//         assert_eq!(mid, expected_mid);

//         let event = parser.event(None).await.expect("event should be produced");
//         match event {
//             Event::GasRefunded {
//                 common,
//                 message_id,
//                 recipient_address,
//                 refunded_amount,
//                 cost,
//             } => {
//                 assert_eq!(message_id, expected_mid);
//                 assert_eq!(common.event_id, "dummy_sig");
//                 assert!(!recipient_address.is_empty());
//                 assert_eq!(refunded_amount.amount, "4242");
//                 assert_eq!(cost.amount, "0");
//             }
//             other => panic!("Expected GasRefunded, got {:?}", other),
//         }
//     }
// }
