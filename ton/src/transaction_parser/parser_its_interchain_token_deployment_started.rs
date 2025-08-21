use crate::boc::its_interchain_token_deployment_started::LogITSInterchainTokenDeploymentStartedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_INTERCHAIN_TOKEN_DEPLOYMENT_STARTED_LOG;
use crate::transaction_parser::common::is_log_emitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{
    CommonEventFields, Event, EventMetadata, InterchainTokenDefinition,
};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserITSInterchainTokenDeploymentStarted {
    log: Option<LogITSInterchainTokenDeploymentStartedMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
    log_position: isize,
}

impl ParserITSInterchainTokenDeploymentStarted {
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
impl Parser for ParserITSInterchainTokenDeploymentStarted {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log_position == -1 {
            return Ok(false);
        }

        if self.log.is_none() {
            self.log = Some(
                LogITSInterchainTokenDeploymentStartedMessage::from_boc_b64(
                    &self.tx.out_msgs[0].message_content.body,
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
        let pos = is_log_emitted(&self.tx, OP_INTERCHAIN_TOKEN_DEPLOYMENT_STARTED_LOG)?;
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

        Ok(Event::ITSInterchainTokenDeploymentStarted {
            common: CommonEventFields {
                r#type: "ITS/INTERCHAIN_TOKEN_DEPLOYMENT_STARTED".to_owned(),
                event_id: format!(
                    "{}-its-interchain-token-deployment-started",
                    tx.hash.clone()
                ),
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
            destination_chain: log.destination_chain,
            token: InterchainTokenDefinition {
                id: format!("0x{}", log.token_id.to_str_radix(16)),
                name: log.token_name,
                symbol: log.token_symbol,
                decimals: log.decimals,
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
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_its_interchain_token_deployment_started::ParserITSInterchainTokenDeploymentStarted;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[20].transactions[3].clone();
        let address = tx.clone().account;

        let mut parser = ParserITSInterchainTokenDeploymentStarted::new(tx, address)
            .await
            .unwrap();

        assert!(parser.check_match().await.unwrap());
        assert!(parser.message_id().await.is_ok());

        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::ITSInterchainTokenDeploymentStarted {
                common,
                destination_chain,
                token,
                message_id,
            } => {
                assert_eq!(message_id, "foo");

                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(
                    token.id,
                    "0xa83f8491782f4edd33810373a6bc95a42ff4a460381d5ee4f86ff33faf2dfbbc"
                );
                assert_eq!(token.symbol, "TONTEST");
                assert_eq!(token.name, "Test token");
                assert_eq!(token.decimals, 9);
                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("Det6b7Uh8FfP7N3e6pqb4guD71ZJj5WxN49Y/QezJQM=")
                );
            }
            _ => panic!("Expected ITSInterchainTokenDeploymentStartedEvent event"),
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
        let mut parser = ParserITSInterchainTokenDeploymentStarted::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.check_match().await.unwrap());
    }
}
