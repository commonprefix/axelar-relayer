use crate::boc::relayer_execute_wrapped::RelayerExecuteWrappedMessage;
use crate::error::TransactionParsingError;
use crate::ton_constants::{
    EXIT_CODE_INSUFFICIENT_GAS, OP_NULLIFIED_SUCCESSFULLY, OP_RELAYER_EXECUTE,
};
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{CannotExecuteMessageReason, CommonEventFields, Event};
use ton_types::ton_types::Trace;
use tonlib_core::TonAddress;

pub struct ParserExecuteInsufficientGas {
    log: Option<RelayerExecuteWrappedMessage>,
    trace: Trace,
    allowed_address: TonAddress,
    _chain_name: String,
}

impl ParserExecuteInsufficientGas {
    pub(crate) async fn new(
        trace: Trace,
        allowed_address: TonAddress,
        chain_name: String,
    ) -> Result<Self, TransactionParsingError> {
        Ok(Self {
            log: None,
            trace,
            allowed_address,
            _chain_name: chain_name,
        })
    }
}

#[async_trait]
impl Parser for ParserExecuteInsufficientGas {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log.is_none() {
            self.log = Some(
                RelayerExecuteWrappedMessage::from_boc_b64(
                    &self.trace.transactions[0].out_msgs[0].message_content.body,
                )
                .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
            );
        }
        Ok(true)
    }

    async fn is_match(&self) -> Result<bool, TransactionParsingError> {
        let txs = &self.trace.transactions;

        if txs.len() >= 3 {
            let tx1 = &txs[1];
            let tx2 = &txs[2];

            if tx2.account == self.allowed_address {
                let opcode = tx1.out_msgs.first().and_then(|msg| msg.opcode).unwrap_or(0);

                if tx1.out_msgs.len() == 1 && opcode == OP_RELAYER_EXECUTE {
                    let exit_code = tx2
                        .description
                        .compute_ph
                        .as_ref()
                        .and_then(|ph| ph.exit_code)
                        .unwrap_or(0) as u32;

                    if exit_code == EXIT_CODE_INSUFFICIENT_GAS {
                        return Ok(true);
                    }
                }
            }
        }

        if txs.len() == 5 && txs[3].in_msg.is_some() {
            if let Some(in_msg4) = &txs[4].in_msg {
                if in_msg4.opcode == Some(OP_NULLIFIED_SUCCESSFULLY) {
                    if let Some(action) = &txs[4].description.action {
                        if action.result_code == 37 {
                            return Ok(true);
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    async fn key(&self) -> Result<MessageMatchingKey, TransactionParsingError> {
        unimplemented!()
    }

    async fn event(&self, _: Option<String>) -> Result<Event, TransactionParsingError> {
        let log = self.log.clone().unwrap();

        let cannot_execute_message_event = Event::CannotExecuteMessageV2 {
            common: CommonEventFields {
                r#type: "CANNOT_EXECUTE_MESSAGE/V2".to_owned(),
                event_id: format!("cannot-execute-task-v2-{}", self.trace.end_lt),
                meta: None,
            },
            message_id: log.message_id,
            source_chain: log.source_chain,
            reason: CannotExecuteMessageReason::InsufficientGas,
            details: self.trace.trace_id.to_string(),
        };

        Ok(cannot_execute_message_event)
    }

    async fn message_id(&self) -> Result<Option<String>, TransactionParsingError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:00194AAD8E422BEDF43FEE746D6D929D369DBAB25468A69D513706EA6978B63A",
        )
        .unwrap();

        let tr = traces[14].clone();
        let mut parser = ParserExecuteInsufficientGas::new(tr, address.clone(), "ton2".to_string())
            .await
            .unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        let event = parser.event(None).await.unwrap();
        match event {
            Event::CannotExecuteMessageV2 { message_id, .. } => {
                assert_eq!(
                    message_id,
                    "0x89f3252fb9ad7003c25471685f48d14c842a2910b9386d35f859694babf7b1cf"
                );
            }
            _ => panic!("Expected CallContract event"),
        }
    }

    #[tokio::test]
    async fn test_no_match() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:00194AAD8E422BEDF43FEE746D6D929D369DBAB25468A69D513706EA6978B63A",
        )
        .unwrap();

        let tr = traces[12].clone();
        let parser = ParserExecuteInsufficientGas::new(tr, address.clone(), "ton2".to_string())
            .await
            .unwrap();
        assert!(!parser.is_match().await.unwrap());
    }

    #[tokio::test]
    async fn test_parser_result_code_37() {
        let traces = fixture_traces();

        let address = TonAddress::from_hex_str(
            "0:00194AAD8E422BEDF43FEE746D6D929D369DBAB25468A69D513706EA6978B63A",
        )
        .unwrap();

        let tr = traces[15].clone();
        let mut parser = ParserExecuteInsufficientGas::new(tr, address.clone(), "ton2".to_string())
            .await
            .unwrap();
        assert!(parser.is_match().await.unwrap());
        parser.parse().await.unwrap();
        let event = parser.event(None).await.unwrap();
        match event {
            Event::CannotExecuteMessageV2 { message_id, .. } => {
                assert_eq!(
                    message_id,
                    "0xbb66ce2ce9f5b1a21d967046101b302b626bcb8b586e7cfa9b9aeb1a4ea2f5ca"
                );
            }
            _ => panic!("Expected CallContract event"),
        }
    }
}
