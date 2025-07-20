use crate::boc::native_gas_added::NativeGasAddedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_ADD_NATIVE_GAS;
use crate::transaction_parser::common::is_log_emmitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserNativeGasAdded {
    log: Option<NativeGasAddedMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserNativeGasAdded {
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
impl Parser for ParserNativeGasAdded {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                NativeGasAddedMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        is_log_emmitted(&self.tx, OP_ADD_NATIVE_GAS, 0)
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        unimplemented!()
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let tx = &self.tx;
        let log = self.log.clone().unwrap();

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
            message_id: self.message_id().await?.unwrap(),
            refund_address: log.refund_address.to_hex(),
            payment: Amount {
                token_id: None,
                amount: log.msg_value.to_string(),
            },
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        let log = self.log.clone().unwrap();
        let addr = format!("0x{}", log.tx_hash);
        Ok(Some(addr))
    }

    async fn needs_message_id(&self) -> bool {
        true
    }

    async fn provides_message_id(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_native_gas_added::ParserNativeGasAdded;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[5].transactions[1].clone();
        let address = tx.clone().account;

        let mut parser = ParserNativeGasAdded::new(tx, address)
            .await
            .unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        assert_eq!(
            parser.message_id().await.unwrap().unwrap(),
            "0x0e6f759f68edb972cc1c5ac28ae44a026567c39d0a67d71de90978a12106a6ba".to_string()
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
                    "0x0e6f759f68edb972cc1c5ac28ae44a026567c39d0a67d71de90978a12106a6ba"
                );
                assert_eq!(
                    refund_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(payment.amount, "299338000");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("hlbJSt6b0kkNh0We16gyIxE5WyDRDltaKIYOmfEZtAs=")
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
        let parser = ParserNativeGasAdded::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
