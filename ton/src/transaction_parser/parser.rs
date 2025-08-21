use super::message_matching_key::MessageMatchingKey;
use crate::error::TransactionParsingError;
use crate::gas_calculator::GasCalculator;
use crate::transaction_parser::common::convert_jetton_to_native;
use crate::transaction_parser::parser_call_contract::ParserCallContract;
use crate::transaction_parser::parser_execute_insufficient_gas::ParserExecuteInsufficientGas;
use crate::transaction_parser::parser_its_interchain_token_deployment_started::ParserITSInterchainTokenDeploymentStarted;
use crate::transaction_parser::parser_its_interchain_transfer::ParserITSInterchainTransfer;
use crate::transaction_parser::parser_its_link_token_started::ParserITSLinkTokenStarted;
use crate::transaction_parser::parser_its_token_metadata_registered::ParserITSTokenMetadataRegistered;
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
    async fn check_match(&mut self) -> Result<bool, crate::error::TransactionParsingError>;
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
    its_address: TonAddress,
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
        let mut its: Vec<Box<dyn Parser>> = Vec::new(); // ITS events that need mapping to call contract
        let mut gas_credit_map: HashMap<MessageMatchingKey, Box<dyn Parser>> = HashMap::new();

        let (total_gas_used, refund_gas_used) = self.gas_used(&trace)?;

        let trace_id = trace.trace_id.clone();
        let message_approved_count = self
            .create_parsers(
                trace,
                &mut parsers,
                &mut call_contract,
                &mut gas_credit_map,
                &mut its,
                self.chain_name.clone(),
            )
            .await?;

        info!(
            "Parsing results: trace_id={} parsers={}, call_contract={}, gas_credit_map={}, its={}",
            trace_id,
            parsers.len(),
            call_contract.len(),
            gas_credit_map.len(),
            its.len()
        );

        if (parsers.len() + call_contract.len() + gas_credit_map.len() + its.len()) == 0 {
            warn!("Trace did not produce any parsers: trace_id={}", trace_id);
        }

        for cc in call_contract.iter().clone() {
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

        for (i, its_parser) in its.iter().enumerate() {
            match call_contract.get(i) {
                Some(contract_parser) => {
                    let message_id = contract_parser.message_id().await?;
                    events.push(its_parser.event(message_id).await?);
                }
                None => {
                    return Err(TransactionParsingError::ITSWithoutPair(format!(
                        "No matching call_contract for ITS index {i}"
                    )));
                }
            }
        }

        for parser in parsers {
            let event = parser.event(None).await?;
            events.push(event);
        }

        self.add_gas_used_and_convert(
            &mut events,
            total_gas_used,
            refund_gas_used,
            message_approved_count,
        )
        .await?;

        Ok(events)
    }
}

impl<PV: PriceViewTrait> TraceParser<PV> {
    pub async fn add_gas_used_and_convert(
        &self,
        events: &mut [Event],
        total_gas_used: u64,
        refund_gas_used: u64,
        message_approved_count: u64,
    ) -> Result<(), TransactionParsingError> {
        for e in events.iter_mut() {
            match e {
                Event::GasCredit { payment, .. } => {
                    if let Some(token_id) = payment.token_id.take() {
                        let amount = BigUint::from_str(payment.amount.as_str())
                            .map_err(|e| TransactionParsingError::Generic(e.to_string()))?;

                        let native = convert_jetton_to_native(token_id, &amount, &self.price_view)
                            .await
                            .map_err(|e| TransactionParsingError::Generic(e.to_string()))?;

                        payment.amount = native.to_string();
                    }
                }

                Event::MessageApproved { cost, .. } => {
                    let per = if message_approved_count == 0 {
                        0
                    } else {
                        total_gas_used / message_approved_count
                    };
                    cost.amount = per.to_string();
                }

                Event::MessageExecuted { cost, .. } => {
                    cost.amount = total_gas_used.to_string();
                }

                Event::GasRefunded { cost, .. } => {
                    cost.amount = refund_gas_used.to_string();
                }

                _ => {}
            }
        }

        Ok(())
    }
}

impl<PV: PriceViewTrait> TraceParser<PV> {
    pub fn new(
        price_view: PV,
        gateway_address: TonAddress,
        gas_service_address: TonAddress,
        its_address: TonAddress,
        gas_calculator: GasCalculator,
        chain_name: String,
    ) -> Self {
        Self {
            price_view,
            gateway_address,
            gas_service_address,
            its_address,
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
        its: &mut Vec<Box<dyn Parser>>,
        chain_name: String,
    ) -> Result<u64, TransactionParsingError> {
        let mut message_approved_count = 0u64;

        let mut parser = ParserExecuteInsufficientGas::new(
            trace.clone(),
            self.gateway_address.clone(),
            chain_name.clone(),
        )
        .await?;
        if parser.check_match().await? {
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
            if parser.check_match().await? {
                info!("ParserCallContract matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                call_contract.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserMessageExecuted::new(tx.clone(), self.gateway_address.clone()).await?;
            if parser.check_match().await? {
                info!("ParserMessageExecuted matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserMessageApproved::new(tx.clone(), self.gateway_address.clone()).await?;
            if parser.check_match().await? {
                info!("ParserMessageApproved matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                message_approved_count += 1;
                continue;
            }
            let mut parser =
                ParserNativeGasPaid::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.check_match().await? {
                info!("ParserNativeGasPaid matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                let key = parser.key().await?;
                gas_credit_map.insert(key, Box::new(parser));
                continue;
            }
            let mut parser =
                ParserNativeGasAdded::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.check_match().await? {
                info!("ParserNativeGasAdded matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserJettonGasAdded::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.check_match().await? {
                info!("ParserJettonGasAdded matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserJettonGasPaid::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.check_match().await? {
                info!("ParserJettonGasPaid matched, trace_id={}", trace.trace_id);
                parser.parse().await?;
                let key = parser.key().await?;
                gas_credit_map.insert(key, Box::new(parser));
                continue;
            }
            let mut parser =
                ParserNativeGasRefunded::new(tx.clone(), self.gas_service_address.clone()).await?;
            if parser.check_match().await? {
                info!(
                    "ParserNativeGasRefunded matched, trace_id={}",
                    trace.trace_id
                );
                parser.parse().await?;
                parsers.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserITSTokenMetadataRegistered::new(tx.clone(), self.its_address.clone()).await?;
            if parser.check_match().await? {
                info!(
                    "ParserITSTokenMetadataRegistered matched, trace_id={}",
                    trace.trace_id
                );
                parser.parse().await?;
                its.push(Box::new(parser));
                continue;
            }
            let mut parser = ParserITSInterchainTokenDeploymentStarted::new(
                tx.clone(),
                self.its_address.clone(),
            )
            .await?;
            if parser.check_match().await? {
                info!(
                    "ParserITSInterchainTokenDeploymentStarted matched, trace_id={}",
                    trace.trace_id
                );
                parser.parse().await?;
                its.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserITSInterchainTransfer::new(tx.clone(), self.its_address.clone()).await?;
            if parser.check_match().await? {
                info!(
                    "ParserITSInterchainTransfer matched, trace_id={}",
                    trace.trace_id
                );
                parser.parse().await?;
                its.push(Box::new(parser));
                continue;
            }
            let mut parser =
                ParserITSLinkTokenStarted::new(tx.clone(), self.its_address.clone()).await?;
            if parser.check_match().await? {
                info!(
                    "ParserITSLinkTokenStarted matched, trace_id={}",
                    trace.trace_id
                );
                parser.parse().await?;
                its.push(Box::new(parser));
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
        let its = TonAddress::from_hex_str(
            "0:000000000000000000000000000000000000000000000000000000000000ffff",
        )
        .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = self::mock_price_view();

        let traces = fixture_traces();
        let parser = TraceParser::new(
            price_view,
            traces[9].transactions[4].account.clone(),
            traces[9].transactions[1].account.clone(),
            its,
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
        let its = TonAddress::from_base64_url("kQD-xq9YjzE6cq10P801OkBA65abvxvID5pnfFjTszltjilk")
            .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone(), its.clone()]);

        let price_view = mock_price_view();
        let traces = fixture_traces();
        let gateway = traces[11].transactions[2].account.clone();

        let parser = TraceParser::new(
            price_view,
            gateway,
            gas_service,
            its,
            calc,
            "ton2".to_string(),
        );
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
    async fn test_its_call_contract_connection() {
        let gateway =
            TonAddress::from_base64_url("kQApTXLsQhuTDGCFT0E0eMFi_c7sZXRghOus4lInGCl5osD9")
                .unwrap();
        let gas_service =
            TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
                .unwrap();
        let its = TonAddress::from_base64_url("kQDdU6MZZX_QYO4RPTMaPJ9kFUdX2474z2yxRvDuhnXZv-aH")
            .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone(), its.clone()]);

        let price_view = mock_price_view();
        let traces = fixture_traces();

        let parser = TraceParser::new(
            price_view,
            gateway,
            gas_service,
            its,
            calc,
            "ton2".to_string(),
        );
        let events = parser.parse_trace(traces[19].clone()).await.unwrap();
        assert_eq!(events.len(), 2);

        match events[0].clone() {
            Event::Call {
                message,
                destination_chain,
                payload,
                ..
            } => {
                assert_eq!(
                    message.message_id,
                    "0xa88a820f8aa9750d0b057efe44e7e16795656157b796250afc0fbf4d23c649e1"
                );
                assert_eq!(message.source_chain, "ton2");
                assert_eq!(
                    message.source_address,
                    "0:dd53a319657fd060ee113d331a3c9f64154757db8ef8cf6cb146f0ee8675d9bf"
                );
                assert_eq!(
                    message.payload_hash,
                    "IQBup4rxdld80lWFcBVi97AwbcGzoxMLL3V3EIFAMLc="
                );
                assert_eq!(
                    message.destination_address,
                    "axelar157hl7gpuknjmhtac2qnphuazv2yerfagva7lsu9vuj2pgn32z22qa26dk4"
                );
                assert_eq!(destination_chain, "axelar");
                assert_eq!(payload, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAJAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACCeDX8nN2b3rX/z4LjntiXsJn6Lj/LQmhA1H7Z76iiMoQ==");
            }
            _ => panic!("Expected Call event"),
        }
        match events[1].clone() {
            Event::ITSTokenMetadataRegistered {
                decimals,
                message_id,
                address,
                ..
            } => {
                assert_eq!(
                    message_id,
                    "0xa88a820f8aa9750d0b057efe44e7e16795656157b796250afc0fbf4d23c649e1"
                );
                assert_eq!(decimals, 9);
                assert_eq!(
                    address,
                    "0:9e0d7f273766f7ad7ff3e0b8e7b625ec267e8b8ff2d09a10351fb67bea288ca1"
                );
            }
            _ => panic!("Expected ITSTokenMetadataRegisteredEvent event"),
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
        let its = TonAddress::from_base64_url("kQD-xq9YjzE6cq10P801OkBA65abvxvID5pnfFjTszltjilk")
            .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = mock_price_view();

        let traces = fixture_traces();
        let parser = TraceParser::new(
            price_view,
            traces[2].transactions[2].account.clone(),
            gas_service,
            its,
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
        let its = TonAddress::from_base64_url("kQD-xq9YjzE6cq10P801OkBA65abvxvID5pnfFjTszltjilk")
            .unwrap();

        let calc = GasCalculator::new(vec![gateway.clone(), gas_service.clone()]);

        let price_view = self::mock_price_view();

        let traces = fixture_traces();
        let parser = TraceParser::new(
            price_view,
            gateway,
            gas_service,
            its,
            calc,
            "ton2".to_string(),
        );
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
            .returning(|_| Ok(Decimal::from_str("0.5").unwrap()));
        price_view
            .expect_get_price()
            .with(eq("TON/USD"))
            .returning(|_| Ok(Decimal::from_str("3").unwrap()));

        price_view
    }
}
