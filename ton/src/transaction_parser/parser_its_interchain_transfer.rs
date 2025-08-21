use crate::boc::its_interchain_transfer::LogITSInterchainTransferMessage;
use crate::error::TransactionParsingError;
use crate::hashing::payload_hash;
use crate::ton_constants::OP_INTERCHAIN_TRANSFER_LOG;
use crate::transaction_parser::common::is_log_emitted;
use crate::transaction_parser::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser::Parser;
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::{Amount, CommonEventFields, Event, EventMetadata};
use ton_types::ton_types::Transaction;
use tonlib_core::TonAddress;

pub struct ParserITSInterchainTransfer {
    log: Option<LogITSInterchainTransferMessage>,
    tx: Transaction,
    allowed_address: TonAddress,
    log_position: isize,
}

impl ParserITSInterchainTransfer {
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
impl Parser for ParserITSInterchainTransfer {
    async fn parse(&mut self) -> Result<bool, TransactionParsingError> {
        if self.log_position < 0 {
            return Ok(false);
        }

        if self.log.is_none() {
            for tx in &self.tx.out_msgs.clone() {
                if tx.destination.is_none() && tx.opcode == Some(OP_INTERCHAIN_TRANSFER_LOG) {
                    self.log = Some(
                        LogITSInterchainTransferMessage::from_boc_b64(&tx.message_content.body)
                            .map_err(|e| TransactionParsingError::BocParsing(e.to_string()))?,
                    );
                    break;
                }
            }
        }
        Ok(true)
    }

    async fn check_match(&mut self) -> Result<bool, TransactionParsingError> {
        if self.tx.account != self.allowed_address {
            return Ok(false);
        }

        let pos = is_log_emitted(&self.tx, OP_INTERCHAIN_TRANSFER_LOG)?;
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

        let hash = if log.data.is_empty() {
            "0".repeat(32)
        } else {
            payload_hash(&log.data).to_string()
        };

        Ok(Event::ITSInterchainTransfer {
            common: CommonEventFields {
                r#type: "ITS/INTERCHAIN_TRANSFER".to_owned(),
                event_id: format!("{}-its-interchain-transfer", tx.hash.clone()),
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
            token_spent: Amount {
                token_id: Some(format!("0x{}", log.token_id.to_str_radix(16))),
                amount: log.jetton_amount.to_string(),
            },
            source_address: log.sender_address.to_hex(),
            destination_address: log.destination_address,
            data_hash: hash,
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
    use crate::transaction_parser::parser_its_interchain_transfer::ParserITSInterchainTransfer;

    #[tokio::test]
    async fn test_parser() {
        let traces = fixture_traces();

        let tx = traces[21].transactions[5].clone();
        let address = tx.clone().account;

        let mut parser = ParserITSInterchainTransfer::new(tx, address).await.unwrap();

        assert!(parser.check_match().await.unwrap());
        assert!(parser.message_id().await.is_ok());

        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::ITSInterchainTransfer {
                common,
                destination_chain,
                message_id,
                token_spent,
                source_address,
                destination_address,
                data_hash,
            } => {
                assert_eq!(message_id, "foo");

                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(
                    token_spent.token_id.unwrap(),
                    "0x12de0c2d53d40473f2f8683f95d825e2fbb36c319c8cdf95f8de30f933db569c"
                );
                assert_eq!(token_spent.amount, "10");
                assert_eq!(
                    source_address,
                    "0:898ad13c059f2a3a69576a010c41af239b487bc555eabeb9a5894deb11299330"
                );
                assert_eq!(
                    destination_address,
                    "0x307837324434383946433931663333303131454334364566613738643337453032644343333335343533"
                );
                assert_eq!(data_hash, "00000000000000000000000000000000");
                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("whzah8/IAGKVzmJNgD5/w9xygsneoYXrMtaluuPp1vs=")
                );
            }
            _ => panic!("Expected ITSInterchainTransfer event"),
        }
    }

    #[tokio::test]
    async fn test_parser_different_position() {
        let traces = fixture_traces();

        let tx = traces[23].transactions[7].clone();
        let address = tx.clone().account;

        let mut parser = ParserITSInterchainTransfer::new(tx, address).await.unwrap();

        assert!(parser.check_match().await.unwrap());
        assert!(parser.message_id().await.is_ok());

        parser.parse().await.unwrap();
        let event = parser.event(Some("foo".to_string())).await.unwrap();
        match event {
            Event::ITSInterchainTransfer {
                common,
                destination_chain,
                message_id,
                token_spent,
                source_address,
                destination_address,
                data_hash,
            } => {
                assert_eq!(message_id, "foo");

                assert_eq!(destination_chain, "avalanche-fuji");
                assert_eq!(
                    token_spent.token_id.unwrap(),
                    "0x448414de01bef5762e7341b7bcdddd806c31989ce5c552b206b314bae4eb8685"
                );
                assert_eq!(token_spent.amount, "100");
                assert_eq!(
                    source_address,
                    "0:1fe0fa0e78288928ca05908c617fb90bdc7edde429e146ca048bdcd31c890e76"
                );
                assert_eq!(
                    destination_address,
                    "0x51990c837551917363e75636d6eb87d7f68dd6c8"
                );
                assert_eq!(data_hash, "00000000000000000000000000000000");
                let meta = &common.meta.as_ref().unwrap();
                assert_eq!(
                    meta.tx_id.as_deref(),
                    Some("+rzq0roTNGHBDmHs09pgaqgNYGpp5NsjBBhyOPbjhPE=")
                );
            }
            _ => panic!("Expected ITSInterchainTransfer event"),
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
        let mut parser = ParserITSInterchainTransfer::new(tx, address.clone())
            .await
            .unwrap();
        assert!(!parser.check_match().await.unwrap());
    }
}
