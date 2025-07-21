use crate::boc::jetton_gas_added::JettonGasAddedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_USER_BALANCE_SUBTRACTED;
use crate::transaction_parser::common::is_log_emmitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserJettonGasAdded {
    log: Option<JettonGasAddedMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserJettonGasAdded {
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
impl Parser for ParserJettonGasAdded {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                JettonGasAddedMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        let candidate = is_log_emmitted(&self.tx, OP_USER_BALANCE_SUBTRACTED, 0)?;

        if !candidate {
            return Ok(false);
        }

        let parsed = JettonGasAddedMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body);
        Ok(parsed.is_ok())
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
        let message_id = match self.message_id().await? {
            Some(id) => id,
            None => return Err(TransactionParsingError::Message("Missing message id".to_string())),
        };

        Ok(Event::GasCredit {
            common: CommonEventFields {
                r#type: "GAS_CREDIT".to_owned(),
                event_id: format!("{}-gas", tx.hash.clone()),
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
            refund_address: log.refund_address.to_hex(),
            payment: Amount {
                token_id: Some(log.minter.to_hex()),
                amount: log.amount.to_string(),
            },
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        let log = match self.log.clone() {
            Some(log) => log,
            None => return Err(TransactionParsingError::Message("Missing log".to_string())),
        };

        let addr = format!("0x{}", log.tx_hash);
        Ok(Some(addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_jetton_gas_added::ParserJettonGasAdded;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[10].transactions[3].clone();
        let address = tx.clone().account;
        let mut parser = ParserJettonGasAdded::new(tx, address).await.unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        assert_eq!(
            parser.message_id().await.unwrap().unwrap(),
            "0xb9ac1cbe75a96a7146a71df1bf5f3ac00668edba0b432d4c5fbe5d59162aced7".to_string()
        );
        let event = parser.event(None).await.unwrap();
        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(
                    message_id,
                    "0xb9ac1cbe75a96a7146a71df1bf5f3ac00668edba0b432d4c5fbe5d59162aced7"
                );
                assert_eq!(
                    refund_address,
                    "0:ed22df34219ae26039fd977d8e419ae14d78b192e9db5dcfa3597899096470d1"
                );
                assert_eq!(payment.amount, "100000000");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("blzE/VLC5oz8yBYjKnSgUMomLj4oecIIiBwXZcxXY+k=")
                );
            }
            _ => panic!("Expected GasCredit event"),
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
        let parser = ParserJettonGasAdded::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
