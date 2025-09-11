use crate::error::TransactionParsingError;
use crate::transaction_parser::discriminators::{CPI_EVENT_DISC, NATIVE_GAS_ADDED_EVENT_DISC};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::{Parser, ParserConfig};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiCompiledInstruction;
use tracing::{debug, warn};

// TODO: Get them from a library?
#[derive(BorshDeserialize, Clone, Debug)]
pub struct NativeGasAddedEvent {
    /// The Gas service config PDA
    pub _config_pda: Pubkey,
    /// Solana transaction signature
    pub tx_hash: [u8; 64],
    /// index of the log
    pub log_index: u64,
    /// The refund address
    pub refund_address: Pubkey,
    /// amount of SOL
    pub gas_fee_amount: u64,
}

pub struct ParserNativeGasAdded {
    signature: String,
    parsed: Option<NativeGasAddedEvent>,
    instruction: UiCompiledInstruction,
    config: ParserConfig,
}

impl ParserNativeGasAdded {
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
                event_type_discriminator: NATIVE_GAS_ADDED_EVENT_DISC,
            },
        })
    }

    fn try_extract_with_config(
        instruction: &UiCompiledInstruction,
        config: ParserConfig,
    ) -> Option<NativeGasAddedEvent> {
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
        match NativeGasAddedEvent::try_from_slice(payload) {
            Ok(event) => {
                debug!("Native Gas Added vent={:?}", event);
                Some(event)
            }
            Err(e) => {
                warn!("failed to parse native gas added event: {:?}", e);
                None
            }
        }
    }
}

#[async_trait]
impl Parser for ParserNativeGasAdded {
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
            "MessageMatchingKey is not available for NativeGasAddedEvent".to_string(),
        ))
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

        Ok(Event::GasCredit {
            common: CommonEventFields {
                r#type: "GAS_CREDIT".to_owned(),
                event_id: format!("{}-gas", self.signature),
                meta: Some(EventMetadata {
                    tx_id: Some(self.signature.to_string()),
                    from_address: None,
                    finalized: None,
                    source_context: None,
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message_id,
            refund_address: parsed.refund_address.to_string(),
            payment: Amount {
                token_id: None,
                amount: parsed.gas_fee_amount.to_string(),
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
//     async fn test_decode_queue_entry_and_parse_gas_credit() {
//         // Build an AddedGas instruction: [CPI_DISC(8)] [ADDED_DISC(8)] [config_pda(32)] [tx_hash(64)] [log_index(8)] [refund_address(32)] [gas_fee_amount(8)]
//         let mut data: Vec<u8> = Vec::new();
//         data.extend_from_slice(&CPI_EVENT_DISC);
//         data.extend_from_slice(&NATIVE_GAS_ADDED_EVENT_DISC);
//         // config_pda
//         data.extend_from_slice(&[0x11; 32]);
//         // tx_hash (64 bytes)
//         data.extend_from_slice(&[0x22; 64]);
//         // log_index
//         data.extend_from_slice(&(5u64).to_le_bytes());
//         // refund_address (dummy pubkey bytes)
//         data.extend_from_slice(&[0x33; 32]);
//         // gas_fee_amount
//         data.extend_from_slice(&(1000u64).to_le_bytes());
//         let ci = UiCompiledInstruction {
//             program_id_index: 4,
//             accounts: vec![3],
//             data: bs58::encode(data).into_string(),
//             stack_height: Some(2),
//         };

//         let mut parser = ParserNativeGasAdded::new("dummy_sig".to_string(), ci)
//             .await
//             .unwrap();

//         assert!(
//             parser.is_match().await.unwrap(),
//             "parser should match gas credit"
//         );
//         assert!(parser.parse().await.unwrap(), "parser should parse message");

//         let event = parser
//             .event(Some("foo".to_string()))
//             .await
//             .expect("event should be produced");
//         println!("{:?}", event);
//         match event {
//             Event::GasCredit {
//                 message_id,
//                 refund_address,
//                 payment,
//                 ..
//             } => {
//                 assert_eq!(message_id, "foo");
//                 assert!(!refund_address.is_empty());
//                 assert!(!payment.amount.is_empty());
//             }
//             other => panic!("Expected GasCredit, got {:?}", other),
//         }
//     }

//     #[tokio::test]
//     async fn test_parser_native_gas_added() {
//         // Reuse the synthetic instruction builder above
//         let mut data: Vec<u8> = Vec::new();
//         data.extend_from_slice(&CPI_EVENT_DISC);
//         data.extend_from_slice(&NATIVE_GAS_ADDED_EVENT_DISC);
//         data.extend_from_slice(&[0xAA; 32]);
//         data.extend_from_slice(&[0xBB; 64]);
//         data.extend_from_slice(&(7u64).to_le_bytes());
//         data.extend_from_slice(&[0xCC; 32]);
//         data.extend_from_slice(&(4242u64).to_le_bytes());
//         let ci = UiCompiledInstruction {
//             program_id_index: 4,
//             accounts: vec![3],
//             data: bs58::encode(data).into_string(),
//             stack_height: Some(2),
//         };

//         let mut parser = ParserNativeGasAdded::new("dummy_sig_2".to_string(), ci)
//             .await
//             .unwrap();
//         assert!(parser.is_match().await.unwrap());
//         parser.parse().await.unwrap();
//         let ev = parser.event(Some("bar".to_string())).await.unwrap();
//         match ev {
//             Event::GasCredit {
//                 message_id,
//                 payment,
//                 ..
//             } => {
//                 assert_eq!(message_id, "bar");
//                 assert_eq!(payment.amount, "4242");
//             }
//             _ => panic!("Expected GasCredit"),
//         }
//     }

// #[tokio::test]
// async fn test_parser_gas_paid() {
//     let solana_transactions = fixture_native_gas_paid();

//     let tx = solana_transactions[0].clone();
//     let ix = tx.ixs[0].instructions[0].clone();
//     let UiInstruction::Compiled(ci) = ix else {
//         panic!("expected compiled instruction")
//     };
//     let mut parser = ParserNativeGasPaid::new(tx.signature.to_string(), ci)
//         .await
//         .unwrap();

//     assert!(parser.is_match().await.unwrap());
//     assert!(parser.message_id().await.is_ok());
//     parser.parse().await.unwrap();
//     let event = parser.event(Some("foo".to_string())).await.unwrap();
//     match event {
//         Event::GasCredit {
//             common,
//             message_id,
//             refund_address,
//             payment,
//         } => {
//             assert_eq!(message_id, "foo");
//             assert_eq!(
//                 refund_address,
//                 "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
//             );
//             assert_eq!(payment.amount, "198639200");

//             let meta = &common.meta.as_ref().unwrap();
//             assert_eq!(
//                 meta.tx_id.as_deref(),
//                 Some("PzeZlujUPePAMw0Fz/eYeCRz11/X/f5YzUjfYXomzS8=")
//             );
//         }
//         _ => panic!("Expected GasCredit event"),
//     }
// }

// #[tokio::test]
// async fn test_no_match() {
//     let solana_transactions = fixture_native_gas_paid();

//     let tx = solana_transactions[0].clone();
//     let ix = tx.ixs[0].instructions[0].clone();
//     let UiInstruction::Compiled(ci) = ix else {
//         panic!("expected compiled instruction")
//     };
//     let parser = ParserNativeGasPaid::new(tx.signature.to_string(), ci)
//         .await
//         .unwrap();
//     assert!(!parser.is_match().await.unwrap());
// }
//}
