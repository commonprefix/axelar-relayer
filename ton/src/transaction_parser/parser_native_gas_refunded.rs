use crate::boc::native_gas_refunded::NativeGasRefundedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::OP_NATIVE_REFUND;
use crate::transaction_parser::common::is_log_emmitted_in_opcode;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserNativeGasRefunded {
    log: Option<NativeGasRefundedMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
}

impl ParserNativeGasRefunded {
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
impl Parser for ParserNativeGasRefunded {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                NativeGasRefundedMessage::from_boc_b64(&self.tx.out_msgs[1].message_content.body)
                    .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn check_match(&mut self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        is_log_emmitted_in_opcode(&self.tx, OP_NATIVE_REFUND, 1)
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
            None => {
                return Err(TransactionParsingError::Message(
                    "Missing message id".to_string(),
                ))
            }
        };

        Ok(Event::GasRefunded {
            common: CommonEventFields {
                r#type: "GAS_REFUNDED".to_owned(),
                event_id: tx.hash.clone(),
                meta: None,
            },
            message_id,
            recipient_address: log.address.to_hex(),
            refunded_amount: Amount {
                token_id: None,
                amount: log.amount.to_string(),
            },
            cost: Amount {
                token_id: None,
                amount: "0".to_string(),
            },
        })
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        if let Some(log) = self.log.clone() {
            let addr = format!("0x{}", log.tx_hash);
            Ok(Some(addr))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use crate::transaction_parser::parser_native_gas_refunded::ParserNativeGasRefunded;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[7].transactions[2].clone();
        let address = tx.clone().account;
        let mut parser = ParserNativeGasRefunded::new(tx, address).await.unwrap();
        assert!(parser.check_match().await.unwrap());
        parser.parse().await.unwrap();
        assert_eq!(
            parser.message_id().await.unwrap().unwrap(),
            "0xeb065d9d930349d0643b946d961ec600f80d5e5f30ab01df6f136243ee5035c2".to_string()
        );
        let event = parser.event(None).await.unwrap();
        match event {
            Event::GasRefunded {
                common,
                message_id,
                recipient_address,
                refunded_amount,
                cost,
            } => {
                assert_eq!(
                    message_id,
                    "0xeb065d9d930349d0643b946d961ec600f80d5e5f30ab01df6f136243ee5035c2"
                );
                assert_eq!(
                    recipient_address,
                    "0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
                );
                assert_eq!(refunded_amount.amount, "266907599");
                assert_eq!(cost.amount, "0");
                assert_eq!(refunded_amount.token_id.as_deref(), None);
                assert_eq!(cost.token_id.as_deref(), None);
                assert!(common.meta.is_none());
            }
            _ => panic!("Expected GasRefunded event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let tx = traces[7].transactions[0].clone();
        let mut parser = ParserNativeGasRefunded::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.check_match().await.unwrap());
    }
}
