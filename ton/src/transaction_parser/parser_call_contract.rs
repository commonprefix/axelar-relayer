use crate::boc::call_contract::CallContractMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_CALL_CONTRACT;
use crate::transaction_parser::common::{hash_to_message_id, is_log_emmitted};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use relayer_base::gmp_api::gmp_types::{CommonEventFields, Event, EventMetadata, GatewayV2Message};
use std::collections::HashMap;
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserCallContract {
    log: Option<CallContractMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
    chain_name: String,
}

impl ParserCallContract {
    pub(crate) async fn new(
        tx: Transaction,
        allowed_address: TonAddress,
        chain_name: String,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            log: None,
            tx,
            allowed_address,
            chain_name,
        })
    }
}

#[async_trait]
impl Parser for ParserCallContract {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                CallContractMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        is_log_emmitted(&self.tx, OP_CALL_CONTRACT, 0)
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        let log = match self.log.clone() {
            Some(log) => log,
            None => return Err(TransactionParsingError::Message("Missing log".to_string())),
        };
        let key = MessageMatchingKey {
            destination_chain: log.destination_chain.clone(),
            destination_address: log.destination_address.clone(),
            payload_hash: log.payload_hash,
        };

        Ok(key)
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let tx = &self.tx;
        let log = match self.log.clone() {
            Some(log) => log,
            None => return Err(TransactionParsingError::Message("Missing log".to_string())),
        };
        let message_id = match self.message_id().await? {
            Some(id) => id,
            None => {
                return Err(TransactionParsingError::Message(
                    "Missing message id".to_string(),
                ))
            }
        };

        let source_context = HashMap::from([
            ("source_address".to_owned(), log.source_address.to_hex()),
            (
                "destination_address".to_owned(),
                log.destination_address.to_string(),
            ),
            (
                "destination_chain".to_owned(),
                log.destination_chain.clone(),
            ),
        ]);

        let decoded = hex::decode(log.payload).map_err(|e| {
            TransactionParsingError::BocParsing(format!("Failed to decode payload: {e}"))
        })?;

        let b64_payload = BASE64_STANDARD.encode(decoded);

        Ok(Event::Call {
            common: CommonEventFields {
                r#type: "CALL".to_owned(),
                event_id: tx.hash.clone(),
                meta: Some(EventMetadata {
                    tx_id: Some(tx.hash.clone()),
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
                source_address: log.source_address.to_hex(),
                destination_address: log.destination_address.to_string(),
                payload_hash: BASE64_STANDARD.encode(log.payload_hash),
            },
            destination_chain: log.destination_chain.clone(),
            payload: b64_payload,
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        Ok(Some(hash_to_message_id(&self.tx.hash).map_err(|e| {
            TransactionParsingError::Message(e.to_string())
        })?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_call_contract::ParserCallContract;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:00194AAD8E422BEDF43FEE746D6D929D369DBAB25468A69D513706EA6978B63A",
        )
        .unwrap();

        let tx = traces[1].transactions[1].clone();
        let mut parser = ParserCallContract::new(tx, address.clone(), "ton2".to_string())
            .await
            .unwrap();
        assert!(parser.is_match().await.unwrap());
        assert_eq!(
            parser.message_id().await.unwrap().unwrap(),
            "0xd60ccda763591b1af5a1771f0913a6851174ef161da21ed7e750a0240db1fd03".to_string()
        );
        parser.parse().await.unwrap();
        let event = parser.event(None).await.unwrap();
        match event {
            Event::Call {
                common,
                message,
                destination_chain,
                payload,
            } => {
                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(payload, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gUmVsYXllciEAAAAAAAAAAAAAAAAA");
                assert_eq!(
                    message.message_id,
                    "0xd60ccda763591b1af5a1771f0913a6851174ef161da21ed7e750a0240db1fd03"
                );
                assert_eq!(message.source_chain, "ton2");
                assert_eq!(
                    message.payload_hash,
                    "rqZSQ2cAD7SgqiCx1PY9qtHtnp33Fj8jCWc2EPLzfUs="
                );
                assert_eq!(
                    message.source_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("1gzNp2NZGxr1oXcfCROmhRF07xYdoh7X51CgJA2x/QM=")
                );
            }
            _ => panic!("Expected CallContract event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:00194AAD8E422BEDF43FEE746D6D929D369DBAB25468A69D513706EA6978B63A",
        )
        .unwrap();
        let tx = traces[1].transactions[0].clone();
        let parser = ParserCallContract::new(tx, address.clone(), "ton2".to_string())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }

    #[tokio::test]
    async fn test_wrong_address() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();

        let tx = traces[1].transactions[1].clone();
        let parser = ParserCallContract::new(tx, address.clone(), "ton2".to_string())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
