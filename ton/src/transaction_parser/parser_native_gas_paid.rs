use crate::boc::native_gas_paid::NativeGasPaidMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::{OP_PAY_GAS, OP_PAY_NATIVE_GAS_FOR_CONTRACT_CALL};
use crate::transaction_parser::common::is_log_emmitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserNativeGasPaid {
    log: Option<NativeGasPaidMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserNativeGasPaid {
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
impl Parser for ParserNativeGasPaid {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                NativeGasPaidMessage::from_boc_b64(&self.tx.out_msgs[0].message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        if is_log_emmitted(&self.tx, OP_PAY_NATIVE_GAS_FOR_CONTRACT_CALL, 0)? {
            return Ok(true);
        }

        is_log_emmitted(&self.tx, OP_PAY_GAS, 0)
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
                token_id: None,
                amount: log.msg_value.to_string(),
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
    use crate::transaction_parser::parser_native_gas_paid::ParserNativeGasPaid;

    #[tokio::test]
    async fn test_parser_native_gas_paid_for_contract_call() {
        let traces = fixture_traces();

        let tx = traces[4].transactions[2].clone();
        let address = tx.clone().account;

        let mut parser = ParserNativeGasPaid::new(tx, address).await.unwrap();
        assert!(parser.is_match().await.unwrap());
        assert_eq!(parser.message_id().await.is_ok(), true);
        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(message_id, "foo");
                assert_eq!(
                    refund_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(payment.amount, "28846800");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("Ptv+ldOh9sTQOvwx23nPD8t6iGmm2RZVgUBXBk/jyrU=")
                );
            }
            _ => panic!("Expected GasCredit event"),
        }
    }

    #[tokio::test]
    async fn test_parser_gas_paid() {
        let traces = fixture_traces();

        let tx = traces[18].transactions[1].clone();
        let address = tx.clone().account;

        let mut parser = ParserNativeGasPaid::new(tx, address).await.unwrap();
        assert!(parser.is_match().await.unwrap());
        assert_eq!(parser.message_id().await.is_ok(), true);
        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::GasCredit {
                common,
                message_id,
                refund_address,
                payment,
            } => {
                assert_eq!(message_id, "foo");
                assert_eq!(
                    refund_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(payment.amount, "198639200");

                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("PzeZlujUPePAMw0Fz/eYeCRz11/X/f5YzUjfYXomzS8=")
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
        let parser = ParserNativeGasPaid::new(tx, address.clone()).await.unwrap();
        assert!(!parser.is_match().await.unwrap());
    }
}
