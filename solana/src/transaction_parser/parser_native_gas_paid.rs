use crate::error::TransactionParsingError;
use crate::transaction_parser::discriminators::{CPI_EVENT_DISC, NATIVE_GAS_PAID_EVENT_DISC};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::{Parser, ParserConfig};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiCompiledInstruction;
use tracing::{debug, warn};

#[derive(BorshDeserialize, Clone, Debug)]
pub struct NativeGasPaidForContractCallEvent {
    /// The Gas service config PDA
    pub config_pda: Pubkey,
    /// Destination chain on the Axelar network
    pub destination_chain: String,
    /// Destination address on the Axelar network
    pub destination_address: String,
    /// The payload hash for the event we're paying for
    pub payload_hash: [u8; 32],
    /// The refund address
    pub refund_address: Pubkey,
    /// Extra parameters to be passed
    pub params: Vec<u8>,
    /// The amount of SOL to send
    pub gas_fee_amount: u64,
}

pub struct ParserNativeGasPaid {
    signature: String,
    parsed: Option<NativeGasPaidForContractCallEvent>,
    instruction: UiCompiledInstruction,
    config: ParserConfig,
}

impl ParserNativeGasPaid {
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
                event_type_discriminator: NATIVE_GAS_PAID_EVENT_DISC,
            },
        })
    }

    fn try_extract_with_config(
        instruction: &UiCompiledInstruction,
        config: ParserConfig,
    ) -> Option<NativeGasPaidForContractCallEvent> {
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
        match NativeGasPaidForContractCallEvent::try_from_slice(payload) {
            Ok(event) => {
                debug!("Native Gas Paid vent={:?}", event);
                Some(event)
            }
            Err(e) => {
                warn!("failed to parse native gas paid event: {:?}", e);
                None
            }
        }
    }
}

#[async_trait]
impl Parser for ParserNativeGasPaid {
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
            destination_address: parsed.destination_address,
            payload_hash: parsed.payload_hash,
        })
    }

    async fn event(&self, message_id: Option<String>) -> Result<Event, TransactionParsingError> {
        let parsed = self
            .parsed
            .clone()
            .ok_or_else(|| TransactionParsingError::Message("Missing parsed".to_string()))?;

        let message_id = message_id
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
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::fixtures::fixture_native_gas_paid;

    use super::*;
    use serde_json::Value;
    use solana_transaction_status::UiInstruction;
    use solana_types::solana_types::SolanaTransaction;

    #[tokio::test]
    async fn test_decode_queue_entry_and_parse_gas_credit() {
        let raw = r#"{"Transaction":{"Solana":{"signature":[251,90,247,104,39,187,71,5,9,30,111,123,198,171,132,91,203,215,187,255,97,59,71,155,244,76,162,237,131,193,151,91,140,187,179,237,149,203,82,62,143,105,184,105,200,43,203,46,82,97,211,190,161,103,86,167,213,242,64,67,143,98,133,9],"timestamp":null,"logs":["Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc invoke [1]","Program log: Instruction: PayNativeForContractCall","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc invoke [2]","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc consumed 2084 of 192307 compute units","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc success","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc consumed 10020 of 200000 compute units","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc success"],"slot":833,"ixs":[{"index":0,"instructions":[{"programIdIndex":4,"accounts":[3],"data":"U657vb47CWbF2XaNghNVTVQvBD23QxUqAGrU7dHrnj7RcypUZyz6HckWMMux9mx4GqQtAaX6TYPvFhQevbZLqUTmfLBEdqYCSyc3tVaP7kVMLXokAkePkHTWuwKsrH81ybEKS74eEahAisn6BnSRcDgL6wDQx2vHUkMrB6MuWxMWC67FuQEBTK7GFsHGNLyEr5TQFSHoo5Wu59xkpuRt3f8VanjQHJ1deGMLbsGL3tVkTP8txGBaWN6Pq5ssWYv2oA3A7","stackHeight":2}]}]}}}
"#;

        let v: Value = serde_json::from_str(raw).expect("valid json");
        let solana_v = v["Transaction"]["Solana"].clone();

        let tx: SolanaTransaction =
            serde_json::from_value::<SolanaTransaction>(solana_v.clone()).unwrap();

        // Derive discriminators from the first 16 bytes of the test data
        let first_ix_data = tx.ixs[0].instructions[0].clone();
        let UiInstruction::Compiled(ci) = first_ix_data else {
            panic!("expected compiled instruction")
        };
        let bytes = bs58::decode(&ci.data).into_vec().unwrap();
        assert!(bytes.len() >= 16);

        let mut parser = ParserNativeGasPaid::new(tx.signature.to_string(), ci)
            .await
            .unwrap();

        assert!(
            parser.is_match().await.unwrap(),
            "parser should match gas credit"
        );
        assert!(parser.parse().await.unwrap(), "parser should parse message");

        let key = parser.key().await.expect("key should be available");
        assert!(!key.destination_chain.is_empty());
        assert!(!key.destination_address.is_empty());

        let event = parser
            .event(Some("foo".to_string()))
            .await
            .expect("event should be produced");
        println!("{:?}", event);
        match event {
            Event::GasCredit {
                message_id,
                refund_address,
                payment,
                ..
            } => {
                assert_eq!(message_id, "foo");
                assert!(!refund_address.is_empty());
                assert!(!payment.amount.is_empty());
            }
            other => panic!("Expected GasCredit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_parser_native_gas_paid() {
        let solana_transactions = fixture_native_gas_paid();

        for tx in solana_transactions {
            let ix = tx.ixs[0].instructions[0].clone();
            let UiInstruction::Compiled(ci) = ix else {
                panic!("expected compiled instruction")
            };
            let mut parser = ParserNativeGasPaid::new(tx.signature.to_string(), ci)
                .await
                .unwrap();
            assert!(parser.is_match().await.unwrap());
            assert!(parser.message_id().await.is_ok());
            parser.parse().await.unwrap();
            // let event = parser.event(Some("foo".to_string())).await.unwrap();
            // match event {
            //     Event::GasCredit {
            //         common,
            //         message_id,
            //         refund_address,
            //         payment,
            //     } => {
            //         assert_eq!(message_id, "foo");
            //         assert_eq!(
            //             refund_address,
            //             "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
            //         );
            //         assert_eq!(payment.amount, "28846800");

            //         let meta = &common.meta.as_ref().unwrap();
            //         assert_eq!(
            //             meta.tx_id.as_deref(),
            //             Some("Ptv+ldOh9sTQOvwx23nPD8t6iGmm2RZVgUBXBk/jyrU=")
            //         );
            //     }
            //     _ => panic!("Expected GasCredit event"),
            // }
        }
    }

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
}
