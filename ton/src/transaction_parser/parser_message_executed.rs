use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{
    Amount, CommonEventFields, Event, EventMetadata, MessageExecutedEventMetadata,
    MessageExecutionStatus,
};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

use crate::boc::nullified_message::NullifiedSuccessfullyMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::{OP_GATEWAY_EXECUTE, OP_NULLIFIED_SUCCESSFULLY};
use crate::transaction_parser::common::is_log_emmitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;

pub struct ParserMessageExecuted {
    log: Option<NullifiedSuccessfullyMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserMessageExecuted {
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
impl Parser for ParserMessageExecuted {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            let in_msg = &self.tx.clone().in_msg.ok_or_else(|| {
                TransactionParsingError::Message("Transaction has no in_msg".to_string())
            })?;
            self.log = Some(
                NullifiedSuccessfullyMessage::from_boc_b64(&in_msg.message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        let mut msg_idx = 1usize;
        let mut second_log = false;
        let first_log = is_log_emmitted(&self.tx, OP_NULLIFIED_SUCCESSFULLY, 0)?;
        if !first_log {
            second_log = is_log_emmitted(&self.tx, OP_NULLIFIED_SUCCESSFULLY, 1)?;
            msg_idx = 0;
        }

        if !second_log && !first_log {
            return Ok(false);
        }

        Ok(self
            .tx
            .out_msgs.get(msg_idx)
            .and_then(|out_msg| out_msg.opcode.as_ref())
            .map(|op| *op == OP_GATEWAY_EXECUTE)
            .unwrap_or(false))
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        unimplemented!()
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let tx = &self.tx;
        let log = match self.log.clone() {
            Some(log) => log,
            None => return Err(TransactionParsingError::Message("Missing log".to_string())),
        };

        Ok(Event::MessageExecuted {
            common: CommonEventFields {
                r#type: "MESSAGE_EXECUTED".to_owned(),
                event_id: tx.hash.clone(),
                meta: Some(MessageExecutedEventMetadata {
                    common_meta: EventMetadata {
                        tx_id: Some(tx.hash.clone()),
                        from_address: None,
                        finalized: None,
                        source_context: None,
                        timestamp: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    },
                    command_id: None,
                    child_message_ids: None,
                    revert_reason: None,
                }),
            },
            message_id: log.message_id.clone(),
            source_chain: log.clone().source_chain,
            status: MessageExecutionStatus::SUCCESSFUL,
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
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
    use crate::transaction_parser::parser_message_executed::ParserMessageExecuted;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[0].transactions[3].clone();
        let address = tx.clone().account;

        let mut parser = ParserMessageExecuted::new(tx, address).await.unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        let event = parser.event(None).await.unwrap();
        match event {
            Event::MessageExecuted {
                common,
                message_id,
                source_chain,
                status,
                cost,
            } => {
                assert_eq!(
                    message_id,
                    "0xf2b741fb0b2c2fcf92aca82395bc65dab4dd8239a12f366d6045755e0b02c2a2-1"
                );
                assert_eq!(source_chain, "avalanche-fuji");
                assert_eq!(status, MessageExecutionStatus::SUCCESSFUL);
                assert_eq!(cost.amount, "0");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(meta.common_meta.tx_id.as_deref(), Some("aa4"));
                assert_eq!(meta.revert_reason.as_deref(), None);
            }
            _ => panic!("Expected MessageExecuted event"),
        }
    }

    #[tokio::test]
    async fn test_parser_b() {
        let traces = fixture_traces();

        let tx = traces[17].transactions[4].clone();
        let address = tx.clone().account;

        let mut parser = ParserMessageExecuted::new(tx, address).await.unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        let event = parser.event(None).await.unwrap();
        match event {
            Event::MessageExecuted {
                common,
                message_id,
                source_chain,
                status,
                cost,
            } => {
                assert_eq!(
                    message_id,
                    "0x78e6b50757198db577c8a1d1f8c33ed039417be7ec176070a61f2e72387a8610-1"
                );
                assert_eq!(source_chain, "avalanche-fuji");
                assert_eq!(status, MessageExecutionStatus::SUCCESSFUL);
                assert_eq!(cost.amount, "0");
                assert_eq!(cost.token_id.as_deref(), None);

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(meta.common_meta.tx_id.as_deref(), Some("6yp4hEyN3u7JNKkbeKFrdgJI/VJqG4hY/roRvT7RNPw="));
                assert_eq!(meta.revert_reason.as_deref(), None);
            }
            _ => panic!("Expected MessageExecuted event"),
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
        let parser = ParserMessageExecuted::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
