use super::message_matching_key::MessageMatchingKey;
use crate::error::TransactionParsingError;
use crate::gas_calculator::GasCalculator;
use crate::transaction_parser::common::convert_jetton_to_native;
use crate::transaction_parser::parser_call_contract::ParserCallContract;
use crate::transaction_parser::parser_execute_insufficient_gas::ParserExecuteInsufficientGas;
use crate::transaction_parser::parser_jetton_gas_added::ParserJettonGasAdded;
use crate::transaction_parser::parser_jetton_gas_paid::ParserJettonGasPaid;
use crate::transaction_parser::parser_message_approved::ParserMessageApproved;
use crate::transaction_parser::parser_message_executed::ParserMessageExecuted;
use crate::transaction_parser::parser_native_gas_added::ParserNativeGasAdded;
use crate::transaction_parser::parser_native_gas_paid::ParserNativeGasPaid;
use crate::transaction_parser::parser_native_gas_refunded::ParserNativeGasRefunded;
use async_trait::async_trait;
use num_bigint::BigUint;
use relayer_base::gmp_api::gmp_types::Event;
use relayer_base::price_view::PriceViewTrait;
use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use ton_types::ton_types::Trace;
use tonlib_core::TonAddress;
use tracing::{info, warn};

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

pub struct TraceParser<PV> {
    price_view: PV,
    gateway_address: TonAddress,
    gas_service_address: TonAddress,
    gas_calculator: GasCalculator,
    chain_name: String,
}

#[cfg_attr(test, mockall::automock)]
pub trait TraceParserTrait {
    fn parse_trace(
        &self,
        trace: Trace,
    ) -> impl Future<Output = Result<Vec<Event>, crate::error::TransactionParsingError>>;
}

impl<PV: PriceViewTrait> TraceParserTrait for TraceParser<PV> {
    async fn parse_trace(&self, trace: Trace) -> Result<Vec<Event>, TransactionParsingError> {
        let mut events: Vec<Event> = Vec::new();
        let mut parsers: Vec<Box<dyn Parser>> = Vec::new();
        let mut call_contract: Vec<Box<dyn Parser>> = Vec::new();
        let mut gas_credit_map: HashMap<MessageMatchingKey, Box<dyn Parser>> = HashMap::new();

        let (total_gas_used, refund_gas_used) = self.gas_used(&trace)?;

        let trace_id = trace.trace_id.clone();
        let message_approved_count = self
            .create_parsers(
                trace,
                &mut parsers,
                &mut call_contract,
                &mut gas_credit_map,
                self.chain_name.clone(),
            )
            .await?;

        info!(
            "Parsing results: trace_id={} parsers={}, call_contract={}, gas_credit_map={}",
            trace_id,
            parsers.len(),
            call_contract.len(),
            gas_credit_map.len()
        );

        if (parsers.len() + call_contract.len() + gas_credit_map.len()) == 0 {
            warn!("Trace did not produce any parsers: trace_id={}", trace_id);
        }

        for cc in call_contract {
            let cc_key = cc.key().await?;
            events.push(cc.event(None).await?);
            if let Some(parser) = gas_credit_map.get(&cc_key) {
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
                Event::GasCredit {
                    common,
                    message_id,
                    refund_address,
                    mut payment,
                } => {
                    let mut p = payment.clone();
                    if let Some(token_id) = p.token_id {
                        let msg_value = convert_jetton_to_native(
                            token_id,
                            &BigUint::from_str(&p.amount)
                                .map_err(|e| TransactionParsingError::Generic(e.to_string()))?,
                            &self.price_view,
                        )
                        .await
                        .map_err(|e| TransactionParsingError::Generic(e.to_string()))?;
                        p.amount = msg_value.to_string();
                        p.token_id = None;
                        payment = p;
                    }
                    Event::GasCredit {
                        common,
                        message_id,
                        refund_address,
                        payment,
                    }
                }
                Event::MessageApproved {
                    common,
                    message,
                    mut cost,
                } => {
                    cost.amount = (total_gas_used / message_approved_count).to_string();
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
                    cost.amount = total_gas_used.to_string();
                    Event::MessageExecuted {
                        common,
                        message_id,
                        source_chain,
                        status,
                        cost,
                    }
                }
                Event::GasRefunded {
                    common,
                    message_id,
                    recipient_address,
                    refunded_amount,
                    mut cost,
                } => {
                    cost.amount = refund_gas_used.to_string();
                    Event::GasRefunded {
                        common,
                        message_id,
                        recipient_address,
                        refunded_amount,
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

impl<PV: PriceViewTrait> TraceParser<PV> {
    pub fn new(
        price_view: PV,
        gateway_address: TonAddress,
        gas_service_address: TonAddress,
        gas_calculator: GasCalculator,
        chain_name: String,
    ) -> Self {
        Self {
            price_view,
            gateway_address,
            gas_service_address,
            gas_calculator,
            chain_name,
        }
    }

    async fn create_parsers(
        &self,
        trace: Trace,
        parsers: &mut Vec<Box<dyn Parser>>,
        call_contract: &mut Vec<Box<dyn Parser>>,
        gas_credit_map: &mut HashMap<MessageMatchingKey, Box<dyn Parser>>,
        chain_name: String,
    ) -> Result<u64, TransactionParsingError> {
        let mut message_approved_count = 0u64;

        let mut parser = ParserExecuteInsufficientGas::new(
            trace.clone(),
            self.gateway_address.clone(),
            chain_name.clone(),
        )
        .await?;
        if parser.is_match().await? {
            info!(
                "ParserExecuteInsufficientGas matched, trace_id={}",
                trace.trace_id
            );
            parser.parse().await?;
            parsers.push(Box::new(parser));
        }

        for tx in trace.transactions {
            let mut parser = ParserCallContract::new(
                tx.clone(),
                self.gateway_address.clone(),
                chain_name.clone(),
            )
            .await?;
            if parser.is_match().await? {
                info!("ParserCallContract matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                call_contract.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserMessageExecuted::new(tx.clone(), self.gateway_address.clone()).await?;
            if parser.is_match().await? {
                info!("ParserMessageExecuted matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserMessageApproved::new(tx.clone(), self.gateway_address.clone()).await?;
            if parser.is_match().await? {
                info!("ParserMessageApproved matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                message_approved_count += 1;
                continue;
            }
            let mut parser =
                ParserNativeGasPaid::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.is_match().await? {
                info!("ParserNativeGasPaid matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                let key = parser.key().await?;
                gas_credit_map.insert(key, Box::new(parser));
                continue;
            }
            let mut parser =
                ParserNativeGasAdded::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.is_match().await? {
                info!("ParserNativeGasAdded matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserJettonGasAdded::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.is_match().await? {
                info!("ParserJettonGasAdded matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserJettonGasPaid::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.is_match().await? {
                info!("ParserJettonGasPaid matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                let key = parser.key().await?;
                gas_credit_map.insert(key, Box::new(parser));
                continue;
            }
            let mut parser =
                ParserNativeGasRefunded::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.is_match().await? {
                info!(
                    "ParserNativeGasRefunded matched, trace_id={}",
                    trace.trace_id
                );
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
        }
        Ok(message_approved_count)
    }

    fn gas_used(&self, trace: &Trace) -> Result<(u64, u64), TransactionParsingError> {
        let total_gas_used = self
            .gas_calculator
            .calc_message_gas(&trace.transactions)
            .map_err(|e| TransactionParsingError::Gas(e.to_string()))?;

        let refund_gas_used = self
            .gas_calculator
            .calc_message_gas_native_gas_refunded(&trace.transactions)
            .map_err(|e| TransactionParsingError::Gas(e.to_string()))?;

        Ok((total_gas_used, refund_gas_used))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures::fixture_traces;
    use mockall::predicate::eq;
    use relayer_base::database::PostgresDB;
    use relayer_base::price_view::MockPriceView;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn test_parser_converted_and_message_id_set() {
        let gateway = TonAddress::from_hex_str(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let gas_service = TonAddress::from_hex_str(
            "0:00000000000000000000000000000000000000000000000000000000000000ff",
        )
        .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = self::mock_price_view();

        let traces = fixture_traces();
        let parser = TraceParser::new(
            price_view,
            traces[9].transactions[4].account.clone(),
            traces[9].transactions[1].account.clone(),
            calc,
            "ton2".to_string(),
        );
        let events = parser.parse_trace(traces[9].clone()).await.unwrap();
        assert_eq!(events.len(), 2);

        match events[0].clone() {
            Event::Call {
                message,
                destination_chain,
                ..
            } => {
                assert_eq!(destination_chain, "ton2");
                assert_eq!(
                    message.message_id,
                    "0xd59014fd585eed8bee519c40d93be23a991fdb7d68a41eb7ad678dc40510e65d"
                );
            }
            _ => panic!("Expected CallContract event"),
        }

        match events[1].clone() {
            Event::GasCredit {
                message_id,
                payment,
                ..
            } => {
                assert_eq!(
                    message_id,
                    "0xd59014fd585eed8bee519c40d93be23a991fdb7d68a41eb7ad678dc40510e65d"
                );
                assert_eq!(payment.amount, "166667");
                assert!(payment.token_id.is_none());
            }
            _ => panic!("Expected GasCredit event"),
        }
    }

    #[tokio::test]
    async fn test_gas_executed() {
        let gateway =
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap();
        let gas_service =
            TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
                .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = mock_price_view();
        let traces = fixture_traces();
        let gateway = traces[11].transactions[2].account.clone();

        let parser = TraceParser::new(price_view, gateway, gas_service, calc, "ton2".to_string());
        let events = parser.parse_trace(traces[11].clone()).await.unwrap();
        assert_eq!(events.len(), 1);

        match events[0].clone() {
            Event::MessageExecuted { cost, .. } => {
                assert_eq!(cost.amount, "42039207");
                assert!(cost.token_id.is_none());
            }
            _ => panic!("Expected CallContract event"),
        }
    }

    #[tokio::test]
    async fn test_gas_approved() {
        let gateway =
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap();
        let gas_service =
            TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
                .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = mock_price_view();

        let traces = fixture_traces();
        let parser = TraceParser::new(
            price_view,
            traces[2].transactions[2].account.clone(),
            gas_service,
            calc,
            "ton2".to_string(),
        );
        let events = parser.parse_trace(traces[2].clone()).await.unwrap();
        assert_eq!(events.len(), 1);

        match events[0].clone() {
            Event::MessageApproved { cost, .. } => {
                assert_eq!(cost.amount, "27244157");
                assert!(cost.token_id.is_none());
            }
            _ => panic!("Expected MessageApproved event"),
        }
    }

    #[tokio::test]
    async fn test_gas_refunded() {
        let gateway =
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap();
        let gas_service =
            TonAddress::from_base64_url("kQCEKDERj88xS-gD7non_TITN-50i4QI8lMukNkqknAX28OJ")
                .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = self::mock_price_view();

        let traces = fixture_traces();
        let parser = TraceParser::new(price_view, gateway, gas_service, calc, "ton2".to_string());
        let events = parser.parse_trace(traces[8].clone()).await.unwrap();
        assert_eq!(events.len(), 1);

        match events[0].clone() {
            Event::GasRefunded { cost, .. } => {
                assert_eq!(cost.amount, "10869279");
                assert!(cost.token_id.is_none());
            }
            _ => panic!("Expected GasRefunded event"),
        }
    }

    fn mock_price_view() -> MockPriceView<PostgresDB> {
        let mut price_view: MockPriceView<PostgresDB> = MockPriceView::new();
        price_view
            .expect_get_price()
            .with(eq(
                "0:1962e375dcf78f97880e9bec4f63e1afe683b4abdd8855d366014c05ff1160e9/USD",
            ))
            .returning(|_| Ok(Decimal::from_str(&"0.5").unwrap()));
        price_view
            .expect_get_price()
            .with(eq("TON/USD"))
            .returning(|_| Ok(Decimal::from_str(&"3").unwrap()));

        price_view
    }
}
