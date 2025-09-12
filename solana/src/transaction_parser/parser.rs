use super::message_matching_key::MessageMatchingKey;
use crate::transaction_parser::parser_call_contract::ParserCallContract;
use crate::transaction_parser::parser_message_approved::ParserMessageApproved;
use crate::transaction_parser::parser_message_executed::ParserMessageExecuted;
use crate::transaction_parser::parser_native_gas_added::ParserNativeGasAdded;
use crate::transaction_parser::parser_native_gas_paid::ParserNativeGasPaid;
use crate::transaction_parser::parser_native_gas_refunded::ParserNativeGasRefunded;
use crate::{
    error::TransactionParsingError,
    transaction_parser::parser_execute_insufficient_gas::ParserExecuteInsufficientGas,
};
use async_trait::async_trait;
use relayer_base::gmp_api::gmp_types::Event;
use relayer_base::price_view::PriceViewTrait;
use relayer_base::utils::ThreadSafe;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiInstruction;
use solana_types::solana_types::SolanaTransaction;
use std::collections::HashMap;
use tracing::{info, warn};

#[derive(Clone, Copy, Debug)]
pub struct ParserConfig {
    pub event_cpi_discriminator: [u8; 8],
    pub event_type_discriminator: [u8; 8],
}

#[async_trait]
pub trait Parser {
    async fn parse(&mut self) -> Result<bool, crate::error::TransactionParsingError>;
    async fn is_match(&self) -> Result<bool, crate::error::TransactionParsingError>;
    async fn key(&self) -> Result<MessageMatchingKey, crate::error::TransactionParsingError>;
    async fn event(
        &self,
        message_id: Option<String>,
    ) -> Result<Event, crate::error::TransactionParsingError>;
    async fn message_id(&self) -> Result<Option<String>, crate::error::TransactionParsingError>;
}

#[derive(Clone)]
pub struct TransactionParser<PV> {
    _price_view: PV,
    _gateway_address: Pubkey,
    _gas_service_address: Pubkey,
    chain_name: String,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TransactionParserTrait: Send + Sync {
    async fn parse_transaction(
        &self,
        transaction: SolanaTransaction,
    ) -> Result<Vec<Event>, TransactionParsingError>;
}

#[async_trait]
impl<PV> TransactionParserTrait for TransactionParser<PV>
where
    PV: PriceViewTrait + ThreadSafe,
{
    async fn parse_transaction(
        &self,
        transaction: SolanaTransaction,
    ) -> Result<Vec<Event>, TransactionParsingError> {
        let mut events: Vec<Event> = Vec::new();
        let mut parsers: Vec<Box<dyn Parser + Send + Sync>> = Vec::new();
        let mut call_contract: Vec<Box<dyn Parser + Send + Sync>> = Vec::new();
        let mut gas_credit_map: HashMap<MessageMatchingKey, Box<dyn Parser + Send + Sync>> =
            HashMap::new();

        let transaction_id = transaction.signature;
        let (message_approved_count, message_executed_count) = self
            .create_parsers(
                transaction.clone(),
                &mut parsers,
                &mut call_contract,
                &mut gas_credit_map,
                self.chain_name.clone(),
            )
            .await?;

        info!(
            "Parsing results: transaction_id={} parsers={}, call_contract={}, gas_credit_map={}",
            transaction_id,
            parsers.len(),
            call_contract.len(),
            gas_credit_map.len()
        );

        if (parsers.len() + call_contract.len() + gas_credit_map.len()) == 0 {
            warn!(
                "Transaction did not produce any parsers: transaction_id={}",
                transaction_id
            );
        }

        for cc in call_contract {
            let cc_key = cc.key().await?;
            events.push(cc.event(None).await?);
            if let Some(parser) = gas_credit_map.remove(&cc_key) {
                let message_id = cc.message_id().await?.ok_or_else(|| {
                    TransactionParsingError::Message("Missing message_id".to_string())
                })?;

                let event = parser.event(Some(message_id)).await?;
                events.push(event);
            }
        }

        for parser in parsers {
            let event = parser.event(None).await?;
            events.push(event);
        }

        let mut parsed_events: Vec<Event> = Vec::new();

        for event in events {
            let event = match event {
                Event::MessageApproved {
                    common,
                    message,
                    mut cost,
                } => {
                    let cost_units = transaction.clone().cost_units;
                    if cost_units == 0 {
                        return Err(TransactionParsingError::Generic(
                            "Cost units for approved not found".to_string(),
                        ));
                    }
                    cost.amount = (transaction
                        .clone()
                        .cost_units
                        .checked_div(message_approved_count))
                    .unwrap_or(0)
                    .to_string();
                    Event::MessageApproved {
                        common,
                        message,
                        cost,
                    }
                }
                Event::MessageExecuted {
                    common,
                    message_id,
                    source_chain,
                    status,
                    mut cost,
                } => {
                    let cost_units = transaction.clone().cost_units;
                    if cost_units == 0 {
                        return Err(TransactionParsingError::Generic(
                            "Cost units for executed not found".to_string(),
                        ));
                    }
                    cost.amount = (transaction
                        .clone()
                        .cost_units
                        .checked_div(message_executed_count))
                    .unwrap_or(0)
                    .to_string();
                    Event::MessageExecuted {
                        common,
                        message_id,
                        source_chain,
                        status,
                        cost,
                    }
                }
                other => other,
            };
            parsed_events.push(event);
        }

        Ok(parsed_events)
    }
}

impl<PV: PriceViewTrait> TransactionParser<PV> {
    pub fn new(
        price_view: PV,
        gateway_address: Pubkey,
        gas_service_address: Pubkey,
        chain_name: String,
    ) -> Self {
        Self {
            _price_view: price_view,
            _gateway_address: gateway_address,
            _gas_service_address: gas_service_address,
            chain_name,
        }
    }

    async fn create_parsers(
        &self,
        transaction: SolanaTransaction,
        parsers: &mut Vec<Box<dyn Parser + Send + Sync>>,
        call_contract: &mut Vec<Box<dyn Parser + Send + Sync>>,
        gas_credit_map: &mut HashMap<MessageMatchingKey, Box<dyn Parser + Send + Sync>>,
        chain_name: String,
    ) -> Result<(u64, u64), TransactionParsingError> {
        let mut index = 0;
        let mut message_approved_count = 0u64;
        let mut message_executed_count = 0u64;

        for group in transaction.ixs.iter() {
            for inst in group.instructions.iter() {
                if let UiInstruction::Compiled(ci) = inst {
                    // Should we check from which account the event was emitted?
                    let mut parser =
                        ParserNativeGasPaid::new(transaction.signature.to_string(), ci.clone())
                            .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserNativeGasPaid matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        let key = parser.key().await?;
                        gas_credit_map.insert(key, Box::new(parser));
                    }

                    let mut parser =
                        ParserNativeGasAdded::new(transaction.signature.to_string(), ci.clone())
                            .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserNativeGasAdded matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        parsers.push(Box::new(parser));
                    }
                    let mut parser =
                        ParserNativeGasRefunded::new(transaction.signature.to_string(), ci.clone())
                            .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserNativeGasRefunded matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        parsers.push(Box::new(parser));
                    }
                    let mut parser = ParserCallContract::new(
                        transaction.signature.to_string(),
                        ci.clone(),
                        chain_name.clone(),
                        index,
                    )
                    .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserCallContract matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        call_contract.push(Box::new(parser));
                    }
                    let mut parser =
                        ParserMessageApproved::new(transaction.signature.to_string(), ci.clone())
                            .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserMessageApproved matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        parsers.push(Box::new(parser));
                        message_approved_count += 1;
                    }
                    let mut parser =
                        ParserMessageExecuted::new(transaction.signature.to_string(), ci.clone())
                            .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserMessageExecuted matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        parsers.push(Box::new(parser));
                        message_executed_count += 1;
                    }
                    let mut parser = ParserExecuteInsufficientGas::new(
                        transaction.signature.to_string(),
                        ci.clone(),
                    )
                    .await?;
                    if parser.is_match().await? {
                        info!(
                            "ParserExecuteInsufficientGas matched, transaction_id={}",
                            transaction.signature
                        );
                        parser.parse().await?;
                        parsers.push(Box::new(parser));
                    }
                    index += 1;
                }
            }
        }

        Ok((message_approved_count, message_executed_count))
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::test_utils::fixtures::fixture_traces;
//     use mockall::predicate::eq;
//     use relayer_base::database::PostgresDB;
//     use relayer_base::price_view::MockPriceView;
//     use rust_decimal::Decimal;

//     #[tokio::test]
//     async fn test_parser_converted_and_message_id_set() {
//         let gateway = TonAddress::from_hex_str(
//             "0:0000000000000000000000000000000000000000000000000000000000000000",
//         )
//         .unwrap();
//         let gas_service = TonAddress::from_hex_str(
//             "0:00000000000000000000000000000000000000000000000000000000000000ff",
//         )
//         .unwrap();

//         let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

//         let price_view = self::mock_price_view();

//         let traces = fixture_traces();
//         let parser = TraceParser::new(
//             price_view,
//             traces[9].transactions[4].account.clone(),
//             traces[9].transactions[1].account.clone(),
//             calc,
//             "ton2".to_string(),
//         );
//         let events = parser.parse_trace(traces[9].clone()).await.unwrap();
//         assert_eq!(events.len(), 2);

//         match events[0].clone() {
//             Event::Call {
//                 message,
//                 destination_chain,
//                 ..
//             } => {
//                 assert_eq!(destination_chain, "ton2");
//                 assert_eq!(
//                     message.message_id,
//                     "0xd59014fd585eed8bee519c40d93be23a991fdb7d68a41eb7ad678dc40510e65d"
//                 );
//             }
//             _ => panic!("Expected CallContract event"),
//         }

//         match events[1].clone() {
//             Event::GasCredit {
//                 message_id,
//                 payment,
//                 ..
//             } => {
//                 assert_eq!(
//                     message_id,
//                     "0xd59014fd585eed8bee519c40d93be23a991fdb7d68a41eb7ad678dc40510e65d"
//                 );
//                 assert_eq!(payment.amount, "166667");
//                 assert!(payment.token_id.is_none());
//             }
//             _ => panic!("Expected GasCredit event"),
//         }
//     }

//     #[tokio::test]
//     async fn test_gas_executed() {
//         let gateway =
//             TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
//                 .unwrap();
//         let gas_service =
//             TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
//                 .unwrap();

//         let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

//         let price_view = mock_price_view();
//         let traces = fixture_traces();
//         let gateway = traces[11].transactions[2].account.clone();

//         let parser = TraceParser::new(price_view, gateway, gas_service, calc, "ton2".to_string());
//         let events = parser.parse_trace(traces[11].clone()).await.unwrap();
//         assert_eq!(events.len(), 1);

//         match events[0].clone() {
//             Event::MessageExecuted { cost, .. } => {
//                 assert_eq!(cost.amount, "42039207");
//                 assert!(cost.token_id.is_none());
//             }
//             _ => panic!("Expected CallContract event"),
//         }
//     }

//     #[tokio::test]
//     async fn test_gas_approved() {
//         let gateway =
//             TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
//                 .unwrap();
//         let gas_service =
//             TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
//                 .unwrap();

//         let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

//         let price_view = mock_price_view();

//         let traces = fixture_traces();
//         let parser = TraceParser::new(
//             price_view,
//             traces[2].transactions[2].account.clone(),
//             gas_service,
//             calc,
//             "ton2".to_string(),
//         );
//         let events = parser.parse_trace(traces[2].clone()).await.unwrap();
//         assert_eq!(events.len(), 1);

//         match events[0].clone() {
//             Event::MessageApproved { cost, .. } => {
//                 assert_eq!(cost.amount, "27244157");
//                 assert!(cost.token_id.is_none());
//             }
//             _ => panic!("Expected MessageApproved event"),
//         }
//     }

//     #[tokio::test]
//     async fn test_gas_refunded() {
//         let gateway =
//             TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
//                 .unwrap();
//         let gas_service =
//             TonAddress::from_base64_url("kQCEKDERj88xS-gD7non_TITN-50i4QI8lMukNkqknAX28OJ")
//                 .unwrap();

//         let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

//         let price_view = self::mock_price_view();

//         let traces = fixture_traces();
//         let parser = TraceParser::new(price_view, gateway, gas_service, calc, "ton2".to_string());
//         let events = parser.parse_trace(traces[8].clone()).await.unwrap();
//         assert_eq!(events.len(), 1);

//         match events[0].clone() {
//             Event::GasRefunded { cost, .. } => {
//                 assert_eq!(cost.amount, "10869279");
//                 assert!(cost.token_id.is_none());
//             }
//             _ => panic!("Expected GasRefunded event"),
//         }
//     }

//     fn mock_price_view() -> MockPriceView<PostgresDB> {
//         let mut price_view: MockPriceView<PostgresDB> = MockPriceView::new();
//         price_view
//             .expect_get_price()
//             .with(eq(
//                 "0:1962e375dcf78f97880e9bec4f63e1afe683b4abdd8855d366014c05ff1160e9/USD",
//             ))
//             .returning(|_| Ok(Decimal::from_str("0.5").unwrap()));
//         price_view
//             .expect_get_price()
//             .with(eq("TON/USD"))
//             .returning(|_| Ok(Decimal::from_str("3").unwrap()));

//         price_view
//     }
// }
