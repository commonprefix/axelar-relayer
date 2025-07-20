use crate::boc::cc_message::TonCCMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_MESSAGE_APPROVED;
use crate::transaction_parser::common::is_log_emmitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{
    Amount, CommonEventFields, Event, EventMetadata, GatewayV2Message, MessageApprovedEventMetadata,
};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserMessageApproved {
    log: Option<TonCCMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserMessageApproved {
    pub(crate) async fn new(
        tx: Transaction,
        allowed_address: TonAddress,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            log: None,
            tx,
            allowed_address,
        })
    }
}

#[async_trait]
impl Parser for ParserMessageApproved {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                TonCCMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        let op_code = format!("0x{:08x}", OP_MESSAGE_APPROVED);
        is_log_emmitted(&self.tx, &op_code, 0)
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        let log = self.log.clone().unwrap();
        let key = MessageMatchingKey {
            destination_chain: log.destination_chain.clone(),
            destination_address: log.destination_address.clone(),
            payload_hash: log.payload_hash,
        };

        Ok(key)
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let tx = &self.tx;
        let log = self.log.clone().unwrap();

        Ok(Event::MessageApproved {
            common: CommonEventFields {
                r#type: "MESSAGE_APPROVED".to_owned(),
                event_id: tx.hash.clone(),
                meta: Some(MessageApprovedEventMetadata {
                    common_meta: EventMetadata {
                        tx_id: Some(tx.hash.clone()),
                        from_address: None,
                        finalized: None,
                        source_context: None,
                        timestamp: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    },
                    command_id: None,
                }),
            },
            message: GatewayV2Message {
                message_id: log.message_id.clone(),
                source_chain: log.source_chain.clone(),
                source_address: log.source_address.clone(),
                destination_address: log.destination_address.clone(),
                payload_hash: hex::encode(log.payload_hash),
            },
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        Ok(None)
    }

    async fn needs_message_id(&self) -> bool {
        false
    }

    async fn provides_message_id(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_message_approved::ParserMessageApproved;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[0].transactions[0].clone();
        let address = tx.clone().account;

        let mut parser = ParserMessageApproved::new(tx, address)
            .await
            .unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        let event = parser.event(None).await.unwrap();
        match event {
            Event::MessageApproved {
                common,
                message,
                cost,
            } => {
                assert_eq!(
                    message.message_id,
                    "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58-1"
                );
                assert_eq!(message.source_chain, "avalanche-fuji");
                assert_eq!(
                    message.payload_hash,
                    "9e01c423ca440c5ec2beecc9d0a152b54fc8e7a416c931b7089eaf221605af17"
                );
                assert_eq!(cost.amount, "0");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(meta.common_meta.tx_id.as_deref(), Some("aa1"));
            }
            _ => panic!("Expected MessageApproved event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let tx = traces[1].transactions[0].clone();
        let parser = ParserMessageApproved::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
