use crate::boc::its_token_metadata_registered::LogTokenMetadataRegisteredMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_REGISTER_TOKEN_METADATA;
use crate::transaction_parser::common::is_log_emmitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{CommonEventFields, Event, EventMetadata};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserITSTokenMetadataRegistered {
    log: Option<LogTokenMetadataRegisteredMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserITSTokenMetadataRegistered {
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
impl Parser for ParserITSTokenMetadataRegistered {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                LogTokenMetadataRegisteredMessage::from_boc_b64(
                    &self.tx.out_msgs[0].message_content.body,
                )
                .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        let candidate = is_log_emmitted(&self.tx, OP_REGISTER_TOKEN_METADATA, 0)?;

        if !candidate {
            return Ok(false);
        }

        let parsed = LogTokenMetadataRegisteredMessage::from_boc_b64(
            &self.tx.out_msgs[0].message_content.body,
        );
        Ok(parsed.is_ok())
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        unimplemented!()
    }

    async fn event(&self, message_id: Option<String>) -> Result<Event, TransactionParsingError> {
        let tx = &self.tx;

        let message_id = if let Some(id) = message_id {
            id
        } else {
            return Err(TransactionParsingError::Message(
                "Missing message_id".to_string(),
            ));
        };

        let log = match self.log.clone() {
            Some(log) => log,
            None => return Err(TransactionParsingError::Message("Missing log".to_string())),
        };

        Ok(Event::ITSTokenMetadataRegisteredEvent {
            common: CommonEventFields {
                r#type: "ITS/TOKEN_METADATA_REGISTERED".to_owned(),
                event_id: format!("{}-its-metadata", tx.hash.clone()),
                meta: Some(EventMetadata {
                    tx_id: Some(tx.hash.clone()),
                    from_address: None,
                    finalized: None,
                    source_context: None,
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }),
            },
            message_id,
            address: log.address.to_hex(),
            decimals: log.decimals,
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_its_token_metadata_registered::ParserITSTokenMetadataRegistered;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[19].transactions[1].clone();
        let address = tx.clone().account;

        let mut parser = ParserITSTokenMetadataRegistered::new(tx, address)
            .await
            .unwrap();
        
        assert!(parser.is_match().await.unwrap());
        assert!(parser.message_id().await.is_ok());
        
        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::ITSTokenMetadataRegisteredEvent {
                common,
                message_id,
                address,
                decimals
            } => {
                assert_eq!(message_id, "foo");
                assert_eq!(
                    address,
                    "0:9e0d7f273766f7ad7ff3e0b8e7b625ec267e8b8ff2d09a10351fb67bea288ca1"
                );
                assert_eq!(decimals, 9);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("fmi15xWaSKCu+gsavto75ZjRgPQzBkoAfNEJ+5Fvh4I=")
                );
            }
            _ => panic!("Expected ITSTokenMetadataRegisteredEvent event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let tx = traces[10].transactions[3].clone(); // ADDED message
        let parser = ParserITSTokenMetadataRegistered::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
