use crate::boc::its_link_token_started::LogITSLinkTokenStartedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_LINK_TOKEN_STARTED_LOG;
use crate::transaction_parser::common::is_log_emitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{CommonEventFields, Event, EventMetadata};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserITSLinkTokenStarted {
    log: Option<LogITSLinkTokenStartedMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
    log_position: isize,
}

impl ParserITSLinkTokenStarted {
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
impl Parser for ParserITSLinkTokenStarted {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log_position < 0 {
            return Ok(false);
        }

        if self.log.is_none() {
            self.log = Some(
                LogITSLinkTokenStartedMessage::from_boc_b64(
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

        let pos = is_log_emitted(&self.tx, OP_LINK_TOKEN_STARTED_LOG)?;
        if pos >= 0 {
            self.log_position = pos;
            return Ok(true);
        };

        Ok(false)
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

        let source_token_address = log.source_token_address.to_hex();
        let source_token_address = if let Some(rest) = source_token_address.strip_prefix("0:") {
            format!("0x{}", rest)
        } else {
            source_token_address.to_string()
        };

        Ok(Event::ITSLinkTokenStarted {
            common: CommonEventFields {
                r#type: "ITS/LINK_TOKEN_STARTED".to_owned(),
                event_id: format!("{}-its-link-token-started", tx.hash.clone()),
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
            token_id: format!("0x{}", log.token_id.to_str_radix(16)),
            destination_chain: log.destination_chain,
            source_token_address,
            destination_token_address: log.destination_token_address,
            token_manager_type: log.token_manager_type,
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
    use crate::transaction_parser::parser_its_link_token_started::ParserITSLinkTokenStarted;
    use relayer_base::gmp_api::gmp_types::TokenManagerType;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[22].transactions[3].clone();
        let address = tx.clone().account;

        let mut parser = ParserITSLinkTokenStarted::new(tx, address).await.unwrap();

        assert!(parser.check_match().await.unwrap());
        assert!(parser.message_id().await.is_ok());

        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::ITSLinkTokenStarted {
                common,
                token_id,
                destination_chain,
                message_id,
                source_token_address,
                destination_token_address,
                token_manager_type,
            } => {
                assert_eq!(message_id, "foo");
                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(
                    token_id,
                    "0x3b68a3e01061c8e033a99697acc6b23e7a829f8d816036f64b11576535e6eeb5"
                );
                assert_eq!(
                    source_token_address,
                    "0x269be316404c6f814e0ae5b3e79f1a1d20100f7ad54abf157e9dd6c49ffcb05b"
                );
                assert_eq!(
                    destination_token_address,
                    "0x307838316536336541384636344645644239383538454236453231373642343331464264313064316543"
                );
                assert_eq!(token_manager_type, TokenManagerType::LockUnlock);
                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("Jfq+NTo1X/4gXQri4fUs4goCq4+zRoz4oHBq+rHI3KM=")
                );
            }
            _ => panic!("Expected ITSLinkTokenStarted event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let tx = traces[20].transactions[3].clone();
        let mut parser = ParserITSLinkTokenStarted::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.check_match().await.unwrap());
    }
}
