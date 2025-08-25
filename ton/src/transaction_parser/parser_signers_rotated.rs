use crate::boc::signers_rotated::LogSignersRotatedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_SIGNERS_ROTATED_LOG;
use crate::transaction_parser::common::{hash_to_message_id, is_log_emitted};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{
    CommonEventFields, Event, EventMetadata, SignersRotatedEventMetadata,
};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserSignersRotated {
    log: Option<LogSignersRotatedMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
    log_position: isize,
}

impl ParserSignersRotated {
    pub(crate) async fn new(
        tx: Transaction,
        allowed_address: TonAddress,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            log: None,
            tx,
            allowed_address,
            log_position: -1,
        })
    }
}

#[async_trait]
impl Parser for ParserSignersRotated {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log_position == -1 {
            return Ok(false);
        }

        if self.log.is_none() {
            self.log = Some(
                LogSignersRotatedMessage::from_boc_b64(
                    &self.tx.out_msgs[self.log_position as usize]
                        .message_content
                        .body,
                )
                .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn check_match(&mut self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }
        let pos = is_log_emitted(&self.tx, OP_SIGNERS_ROTATED_LOG)?;
        if pos >= 0 {
            self.log_position = pos;
            return Ok(true);
        };

        Ok(false)
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        unimplemented!()
    }

    async fn event(&self, _message_id: Option<String>) -> Result<Event, TransactionParsingError> {
        let tx = &self.tx;
        let message_id = match self.message_id().await? {
            Some(id) => id,
            None => {
                return Err(TransactionParsingError::Message(
                    "Missing message id".to_string(),
                ))
            }
        };

        let log = match self.log.clone() {
            Some(log) => log,
            None => return Err(TransactionParsingError::Message("Missing log".to_string())),
        };

        Ok(Event::SignersRotated {
            common: CommonEventFields {
                r#type: "SIGNERS_ROTATED".to_owned(),
                event_id: format!("{}-signers-rotated", tx.hash.clone()),
                meta: Some(SignersRotatedEventMetadata {
                    common_meta: EventMetadata {
                        tx_id: Some(tx.hash.clone()),
                        from_address: None,
                        finalized: None,
                        source_context: None,
                        timestamp: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    },
                    signers_hash: Some(log.signers_hash),
                    epoch: Some(log.epoch),
                }),
            },
            message_id,
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
    use crate::transaction_parser::parser_signers_rotated::ParserSignersRotated;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[24].transactions[1].clone();
        let address = tx.clone().account;

        let mut parser = ParserSignersRotated::new(tx, address).await.unwrap();

        assert!(parser.check_match().await.unwrap());
        assert!(parser.message_id().await.is_ok());

        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::SignersRotated { common, message_id } => {
                assert_eq!(
                    message_id,
                    "0x409645b0e14243e7344db15d1b1bbccd1a2f74bde0dccfc7b0a777f4c340d02f"
                );

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.common_meta.tx_id.as_deref(),
                    Some("QJZFsOFCQ+c0TbFdGxu8zRovdL3g3M/HsKd39MNA0C8=")
                );
                assert_eq!(meta.epoch, Some(2u64));
                assert_eq!(
                    meta.signers_hash,
                    Some(
                        "0x4b163171177cefe9be70322b61eb0bf141920bb8f6faea9c79271a5c331aacd5"
                            .to_string()
                    )
                );
            }
            _ => panic!("Expected SignersRotated event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let tx = traces[20].transactions[1].clone();
        let mut parser = ParserSignersRotated::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.check_match().await.unwrap());
    }
}
