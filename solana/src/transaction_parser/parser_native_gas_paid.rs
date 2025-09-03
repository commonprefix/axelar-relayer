use crate::error::TransactionParsingError;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiInstruction;
use solana_types::solana_types::SolanaTransaction;

#[derive(Clone)]
struct ParsedGasCredit {
    destination_chain: String,
    destination_address: String,
    payload_hash: [u8; 32],
    refund_address: Pubkey,
    gas_fee_amount: u64,
}

pub struct ParserNativeGasPaid {
    parsed: Option<ParsedGasCredit>,
    tx: SolanaTransaction,
    allowed_program: Pubkey,
}

impl ParserNativeGasPaid {
    pub(crate) async fn new(
        tx: SolanaTransaction,
        allowed_program: Pubkey,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            parsed: None,
            tx,
            allowed_program,
        })
    }

    fn try_extract(tx: &SolanaTransaction) -> Option<ParsedGasCredit> {
        for group in tx.ixs.iter() {
            for inst in group.instructions.iter() {
                if let UiInstruction::Compiled(ci) = inst {
                    let bytes = match bs58::decode(&ci.data).into_vec() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if bytes.len() < 16 {
                        continue;
                    }

                    let mut i: usize = 16;

                    fn take_slice<'a>(
                        bytes: &'a [u8],
                        i: &mut usize,
                        len: usize,
                    ) -> Option<&'a [u8]> {
                        if *i + len > bytes.len() {
                            None
                        } else {
                            let out = &bytes[*i..*i + len];
                            *i += len;
                            Some(out)
                        }
                    }

                    fn read_u32(bytes: &[u8], i: &mut usize) -> Option<u32> {
                        let s = take_slice(bytes, i, 4)?;
                        let mut lenb = [0u8; 4];
                        lenb.copy_from_slice(s);
                        Some(u32::from_le_bytes(lenb))
                    }

                    fn read_string(bytes: &[u8], i: &mut usize) -> Option<String> {
                        let len = read_u32(bytes, i)? as usize;
                        let s = take_slice(bytes, i, len)?;
                        Some(std::str::from_utf8(s).ok()?.to_string())
                    }

                    fn read_vec_u8(bytes: &[u8], i: &mut usize) -> Option<Vec<u8>> {
                        let len = read_u32(bytes, i)? as usize;
                        let s = take_slice(bytes, i, len)?;
                        Some(s.to_vec())
                    }

                    fn read_pubkey(bytes: &[u8], i: &mut usize) -> Option<Pubkey> {
                        let s = take_slice(bytes, i, 32)?;
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(s);
                        Some(Pubkey::new_from_array(arr))
                    }

                    let _config_pda = match read_pubkey(&bytes, &mut i) {
                        Some(v) => v,
                        None => continue,
                    };
                    let destination_chain = match read_string(&bytes, &mut i) {
                        Some(v) => v,
                        None => continue,
                    };
                    let destination_address = match read_string(&bytes, &mut i) {
                        Some(v) => v,
                        None => continue,
                    };
                    let payload_hash = match take_slice(&bytes, &mut i, 32) {
                        Some(s) => {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(s);
                            arr
                        }
                        None => continue,
                    };
                    let refund_address = match read_pubkey(&bytes, &mut i) {
                        Some(v) => v,
                        None => continue,
                    };
                    let _params = match read_vec_u8(&bytes, &mut i) {
                        Some(v) => v,
                        None => continue,
                    };
                    let gas_fee_amount = match take_slice(&bytes, &mut i, 8) {
                        Some(s) => {
                            let mut gasb = [0u8; 8];
                            gasb.copy_from_slice(s);
                            u64::from_le_bytes(gasb)
                        }
                        None => continue,
                    };

                    return Some(ParsedGasCredit {
                        destination_chain,
                        destination_address,
                        payload_hash,
                        refund_address,
                        gas_fee_amount,
                    });
                }
            }
        }

        None
    }
}

#[async_trait]
impl Parser for ParserNativeGasPaid {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.parsed.is_none() {
            self.parsed = Self::try_extract(&self.tx);
        }
        Ok(self.parsed.is_some())
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        Ok(Self::try_extract(&self.tx).is_some())
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        let parsed = self
            .parsed
            .as_ref()
            .cloned()
            .or_else(|| Self::try_extract(&self.tx))
            .ok_or_else(|| {
                TransactionParsingError::Message("Missing parsed gas credit".to_string())
            })?;

        Ok(MessageMatchingKey {
            destination_chain: parsed.destination_chain,
            destination_address: parsed.destination_address,
            payload_hash: parsed.payload_hash,
        })
    }

    async fn event(&self, message_id: Option<String>) -> Result<Event, TransactionParsingError> {
        let parsed = self
            .parsed
            .as_ref()
            .cloned()
            .or_else(|| Self::try_extract(&self.tx))
            .ok_or_else(|| {
                TransactionParsingError::Message("Missing parsed gas credit".to_string())
            })?;

        let message_id = message_id
            .ok_or_else(|| TransactionParsingError::Message("Missing message_id".to_string()))?;

        Ok(Event::GasCredit {
            common: CommonEventFields {
                r#type: "GAS_CREDIT".to_owned(),
                event_id: format!("{}-gas", self.tx.signature),
                meta: Some(EventMetadata {
                    tx_id: Some(self.tx.signature.to_string()),
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
    use super::*;
    use serde_json::Value;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_decode_queue_entry_and_parse_gas_credit() {
        let raw = r#"{"Transaction":{"Solana":{"signature":[160,139,158,158,128,241,204,40,48,84,238,157,39,226,190,185,96,102,54,157,166,184,222,97,176,247,188,208,252,32,139,141,215,41,104,56,10,132,186,151,63,217,183,90,34,63,35,139,56,25,243,83,235,65,77,124,34,231,154,152,100,181,108,12],"timestamp":"2025-09-03T16:10:00.723271Z","logs":["Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc invoke [1]","Program log: Instruction: PayNativeForContractCall","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc invoke [2]","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc consumed 2084 of 192465 compute units","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc success","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc consumed 9862 of 200000 compute units","Program 7RdSDLUUy37Wqc6s9ebgo52AwhGiw4XbJWZJgidQ1fJc success"],"slot":25782,"ixs":[{"index":0,"instructions":[{"programIdIndex":4,"accounts":[3],"data":"9K93pGwFHUmnecEp6h1ZtRK5LYv5MLSbJPQ1ZeViVDgJwV3DNyhs5fodWcDTrKdxwhsnwCWyM6DXeM4ibUHVkXU67a1ctDFPteu6XWFKJDeuhaB1zdzreV6TzzFPnvrjEPYhkjm1wrkioWBRMwPVEBwqLLVGp875afAEW7UUFfwpBpakQXkpdhCW76ihGz8SkunrQYduzRW52nQ9RUALZZUNY5FTjJkkTqi3NgcA8qNgUigZQeJ4PpKvX","stackHeight":2}]}]}}}"#;

        let v: Value = serde_json::from_str(raw).expect("valid json");
        let solana_v = v["Transaction"]["Solana"].clone();

        // Try direct deserialization first
        let tx: SolanaTransaction =
            match serde_json::from_value::<SolanaTransaction>(solana_v.clone()) {
                Ok(t) => t,
                Err(_) => {
                    // Manual fallback for constructing a SolanaTransaction
                    use chrono::DateTime;
                    use chrono::Utc;
                    use solana_sdk::signature::Signature;

                    let sig_bytes: Vec<u8> =
                        serde_json::from_value(solana_v["signature"].clone()).unwrap();
                    let sig_b58 = bs58::encode(sig_bytes).into_string();
                    let signature = Signature::from_str(&sig_b58).unwrap();

                    let logs: Vec<String> =
                        serde_json::from_value(solana_v["logs"].clone()).unwrap();
                    let slot: i64 = serde_json::from_value(solana_v["slot"].clone()).unwrap();
                    let ixs = serde_json::from_value(solana_v["ixs"].clone()).unwrap();

                    SolanaTransaction {
                        signature,
                        timestamp: Some(
                            DateTime::parse_from_rfc3339("2025-09-03T16:10:00.723271Z")
                                .unwrap()
                                .with_timezone(&Utc),
                        ),
                        logs,
                        slot,
                        ixs,
                    }
                }
            };

        let allowed_program = Pubkey::new_unique();
        let mut parser = ParserNativeGasPaid::new(tx, allowed_program).await.unwrap();

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
}
