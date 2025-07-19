/*!

Trace Parser for TON Transactions: load a trace from TON chain and return all possible transactions
that a Relayer could be interested in.

The `TraceTransactions` struct contains four categorized vectors:
 - `call_contract`: Transactions invoking the call contract interface.
 - `message_approved`: Approved cross-chain messages.
 - `executed`: Executed transactions confirmed via the gateway.
 - `gas_credit`: Transactions paying for native gas usage.

# Adding a New Trace Type

To support parsing of a new transaction type:
 1. **Define a struct** representing the new on-chain log message (if not already present).
    Implement a `from_boc_b64` method for decoding BOC-encoded body content.

 2. **Extend the `LogMessage` enum** with a new variant for your message.

 3. **Add a new `is_<your_type>` helper** function to detect matching transactions
    based on opcode and output message index.

 4. **Extend the `from_trace` method** in the `impl ParseTrace for TraceTransactions` block:
    - Use your `is_<your_type>` check.
    - Decode the message body using your parser.
    - Push a new `ParsedTransaction` with the corresponding `LogMessage` variant.

# A Word on Gas Credit

Gas Credit transactions need to be mapped to corresponding Call Contract transactions. Here,
we copied the logic from EVM relayers, where for each trace (what they would call a transaction)
we extract all Call Contract transactions (what they would call log events), as well as all Gas
Credit transactions.

Then, these transactions are matched by their destination chain, destination address, and payload hash.
If a match is found, the Gas Credit transaction is removed from the list of Call Contract transactions,
and the `message_id` field is set to the hash of the Call Contract transaction.

This means that a GasCredit can be lost. The reason is that, unlike in EVM, we cannot deduce
a messageId from the trace itself - we can only deduce it if its matched to a ContractCall. However,
this is similar to how EVM relayers work. Go and try to send multiple or mismatched Gas Credit
events in a SenderReceiver, and you'll see that the relayer will only send what it can
actually match.

# Usage Example

Look at test_parse_trace test for an example.

*/

use crate::boc::call_contract::CallContractMessage;
use crate::boc::cc_message::TonCCMessage;
use crate::boc::jetton_gas_added::JettonGasAddedMessage;
use crate::boc::jetton_gas_paid::JettonGasPaidMessage;
use crate::boc::native_gas_added::NativeGasAddedMessage;
use crate::boc::native_gas_paid::NativeGasPaidMessage;
use crate::boc::native_gas_refunded::NativeGasRefundedMessage;
use crate::boc::nullified_message::NullifiedSuccessfullyMessage;
use crate::error::TONRpcError::DataError;
use crate::error::{BocError, TONRpcError};
use crate::parse_trace::LogMessage::{Approved, CallContract, Executed};
use crate::ton_constants::{
    OP_ADD_NATIVE_GAS, OP_CALL_CONTRACT, OP_GATEWAY_EXECUTE, OP_MESSAGE_APPROVED, OP_NATIVE_REFUND,
    OP_NULLIFIED_SUCCESSFULLY, OP_PAY_NATIVE_GAS_FOR_CONTRACT_CALL, OP_USER_BALANCE_SUBTRACTED,
};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use ton_types::ton_types::{Trace, Transaction};
use std::collections::HashMap;
use tracing::info;

#[derive(Eq, Hash, PartialEq)]
struct MessageMatchingKey {
    destination_chain: String,
    destination_address: String,
    payload_hash: [u8; 32],
}

pub struct TraceTransactions {
    pub call_contract: Vec<ParsedTransaction>,
    pub(crate) message_approved: Vec<ParsedTransaction>,
    pub(crate) executed: Vec<ParsedTransaction>,
    pub(crate) gas_credit: Vec<ParsedTransaction>,
    pub(crate) gas_added: Vec<ParsedTransaction>,
    pub(crate) gas_refunded: Vec<ParsedTransaction>,
}

#[derive(Clone)]
pub enum LogMessage {
    Approved(TonCCMessage),
    Executed(NullifiedSuccessfullyMessage),
    CallContract(CallContractMessage),
    NativeGasPaid(NativeGasPaidMessage),
    JettonGasPaid(JettonGasPaidMessage),
    NativeGasAdded(NativeGasAddedMessage),
    JettonGasAdded(JettonGasAddedMessage),
    NativeGasRefunded(NativeGasRefundedMessage),
}

pub struct ParsedTransaction {
    pub(crate) transaction: Transaction,
    pub(crate) log_message: Option<LogMessage>,
    pub(crate) message_id: Option<String>,
}

pub trait ParseTrace {
    fn from_trace(trace: Trace) -> Result<TraceTransactions, BocError>;
}

fn is_log_emmitted(tx: &Transaction, op_code: &str, out_msg_log_index: usize) -> bool {
    Some(tx)
        .and_then(|tx| tx.in_msg.as_ref())
        .and_then(|in_msg| in_msg.opcode.as_ref())
        .filter(|opcode| opcode == &op_code)
        .and_then(|_| tx.out_msgs.get(out_msg_log_index))
        .map(|msg| msg.destination.is_none())
        .unwrap_or(false)
}

fn is_message_approved(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_MESSAGE_APPROVED);
    is_log_emmitted(tx, &op_code, 0)
}

fn is_call_contract(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_CALL_CONTRACT);
    is_log_emmitted(tx, &op_code, 0)
}

fn is_executed(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_NULLIFIED_SUCCESSFULLY);
    if !is_log_emmitted(tx, &op_code, 1) {
        return false;
    }

    let op_code = format!("0x{:08x}", OP_GATEWAY_EXECUTE);

    tx.out_msgs
        .first()
        .and_then(|out_msg| out_msg.opcode.as_ref())
        .map(|op| op == &op_code)
        .unwrap_or(false)
}

fn is_native_gas_paid(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_PAY_NATIVE_GAS_FOR_CONTRACT_CALL);
    is_log_emmitted(tx, &op_code, 0)
}

fn is_native_gas_added(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_ADD_NATIVE_GAS);
    is_log_emmitted(tx, &op_code, 0)
}

fn is_native_gas_refunded(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_NATIVE_REFUND);
    is_log_emmitted(tx, &op_code, 1)
}

fn is_jetton_gas_paid(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_USER_BALANCE_SUBTRACTED);
    let candidate = is_log_emmitted(tx, &op_code, 0);

    if !candidate {
        return false;
    }

    let parsed = JettonGasPaidMessage::from_boc_b64(&tx.out_msgs[0].message_content.body);
    parsed.is_ok()
}

fn is_jetton_gas_added(tx: &Transaction) -> bool {
    let op_code = format!("0x{:08x}", OP_USER_BALANCE_SUBTRACTED);
    let candidate = is_log_emmitted(tx, &op_code, 0);

    if !candidate {
        return false;
    }

    let parsed = JettonGasAddedMessage::from_boc_b64(&tx.out_msgs[0].message_content.body);
    parsed.is_ok()
}

fn hash_to_message_id(hash: &str) -> Result<String, TONRpcError> {
    let hash = BASE64_STANDARD
        .decode(hash)
        .map_err(|e| DataError(e.to_string()))?;
    Ok(format!("0x{}", hex::encode(hash).to_lowercase()))
}

fn gas_credit_map_to_vec(
    call_contract: &Vec<ParsedTransaction>,
    mut map: HashMap<MessageMatchingKey, ParsedTransaction>,
) -> Vec<ParsedTransaction> {
    let mut credit_vec: Vec<ParsedTransaction> = Vec::new();
    for cc_tx in call_contract {
        if let Some(LogMessage::CallContract(call_contract_msg)) = &cc_tx.log_message {
            let key = MessageMatchingKey {
                destination_chain: call_contract_msg.destination_chain.clone(),
                destination_address: call_contract_msg.destination_address.clone(),
                payload_hash: call_contract_msg.payload_hash,
            };

            if let Some(mut gas_credit_tx) = map.remove(&key) {
                let hash = cc_tx.transaction.hash.clone();
                gas_credit_tx.message_id = hash_to_message_id(&hash).ok();
                credit_vec.push(gas_credit_tx);
            }
        }
    }

    credit_vec
}

impl ParseTrace for TraceTransactions {
    fn from_trace(trace: Trace) -> Result<TraceTransactions, BocError> {
        let mut call_contract: Vec<ParsedTransaction> = Vec::new();
        let mut message_approved: Vec<ParsedTransaction> = Vec::new();
        let mut executed: Vec<ParsedTransaction> = Vec::new();
        let mut gas_credit_map: HashMap<MessageMatchingKey, ParsedTransaction> = HashMap::new();
        let mut gas_added: Vec<ParsedTransaction> = Vec::new();
        let mut gas_refunded: Vec<ParsedTransaction> = Vec::new();

        for tx in trace.transactions {
            if is_message_approved(&tx) {
                message_approved.push(ParsedTransaction {
                    log_message: Option::from(Approved(TonCCMessage::from_boc_b64(
                        &tx.out_msgs[0].message_content.body,
                    )?)),
                    transaction: tx,
                    message_id: None,
                });
            } else if is_call_contract(&tx) {
                call_contract.push(ParsedTransaction {
                    log_message: Option::from(CallContract(CallContractMessage::from_boc_b64(
                        &tx.out_msgs[0].message_content.body,
                    )?)),
                    message_id: hash_to_message_id(&tx.hash).ok(),
                    transaction: tx,
                });
            } else if is_executed(&tx) {
                let in_msg = tx.in_msg.as_ref().ok_or(BocError::BocParsingError(
                    "Missing in_msg for executed transaction".into(),
                ))?;
                let message = &in_msg.message_content.body;

                executed.push(ParsedTransaction {
                    log_message: Option::from(Executed(
                        NullifiedSuccessfullyMessage::from_boc_b64(message)?,
                    )),
                    transaction: tx,
                    message_id: None,
                });
            } else if is_native_gas_paid(&tx) {
                let out_msg = &tx.out_msgs[0];
                let msg = NativeGasPaidMessage::from_boc_b64(&out_msg.message_content.body)?;
                let key = MessageMatchingKey {
                    destination_chain: msg.destination_chain.clone(),
                    destination_address: msg.destination_address.clone(),
                    payload_hash: msg.payload_hash,
                };
                gas_credit_map.insert(
                    key,
                    ParsedTransaction {
                        log_message: Option::from(LogMessage::NativeGasPaid(msg)),
                        transaction: tx,
                        message_id: None,
                    },
                );
            } else if is_native_gas_added(&tx) {
                let out_msg = &tx.out_msgs[0];
                let msg = NativeGasAddedMessage::from_boc_b64(&out_msg.message_content.body)?;
                let addr = format!("0x{}", msg.tx_hash);

                gas_added.push(ParsedTransaction {
                    log_message: Option::from(LogMessage::NativeGasAdded(msg.clone())),
                    message_id: Some(addr),
                    transaction: tx,
                });
            } else if is_jetton_gas_added(&tx) {
                let out_msg = &tx.out_msgs[0];
                let msg = JettonGasAddedMessage::from_boc_b64(&out_msg.message_content.body)?;
                let addr = format!("0x{}", msg.tx_hash);

                gas_added.push(ParsedTransaction {
                    log_message: Option::from(LogMessage::JettonGasAdded(msg.clone())),
                    message_id: Some(addr),
                    transaction: tx,
                });
            } else if is_native_gas_refunded(&tx) {
                let out_msg = &tx.out_msgs[1];
                let msg = NativeGasRefundedMessage::from_boc_b64(&out_msg.message_content.body)?;
                let addr = format!("0x{}", msg.tx_hash);

                gas_refunded.push(ParsedTransaction {
                    log_message: Option::from(LogMessage::NativeGasRefunded(msg.clone())),
                    message_id: Some(addr),
                    transaction: tx,
                });
            } else if is_jetton_gas_paid(&tx) {
                let out_msg = &tx.out_msgs[0];
                let msg = JettonGasPaidMessage::from_boc_b64(&out_msg.message_content.body)?;
                let key = MessageMatchingKey {
                    destination_chain: msg.destination_chain.clone(),
                    destination_address: msg.destination_address.clone(),
                    payload_hash: msg.payload_hash,
                };
                gas_credit_map.insert(
                    key,
                    ParsedTransaction {
                        log_message: Option::from(LogMessage::JettonGasPaid(msg)),
                        transaction: tx,
                        message_id: None,
                    },
                );
            }
        }

        let gas_credit = gas_credit_map_to_vec(&call_contract, gas_credit_map);

        info!("Parsed trace id {} and found call_contract:{}, message_approved:{}, executed:{}, gas_credit:{}, gas_added:{}, gas_refunded:{}", 
            trace.trace_id, 
            call_contract.len(), 
            message_approved.len(), 
            executed.len(), 
            gas_credit.len(), 
            gas_added.len(), 
            gas_refunded.len());

        Ok(TraceTransactions {
            call_contract,
            message_approved,
            executed,
            gas_credit,
            gas_added,
            gas_refunded,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::call_contract::CallContractMessage;
    use crate::boc::native_gas_paid::NativeGasPaidMessage;
    use crate::parse_trace::{LogMessage, ParseTrace, TraceTransactions};
    use crate::test_utils::fixtures::fixture_traces;
    use num_bigint::BigUint;
    use std::collections::HashMap;
    use tonlib_core::TonAddress;

    #[test]
    fn test_parse_trace() {
        let traces = fixture_traces();

        let trace_transactions = TraceTransactions::from_trace(traces[0].clone()).unwrap();

        assert_eq!(trace_transactions.executed.len(), 1);
        let parsed_tx = &trace_transactions.executed[0];
        assert_eq!(parsed_tx.transaction.hash, "aa4");
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::Executed(_))
        ));

        assert_eq!(trace_transactions.message_approved.len(), 1);
        let parsed_tx = &trace_transactions.message_approved[0];
        assert_eq!(parsed_tx.transaction.hash, "aa1");
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::Approved(_))
        ));

        assert_eq!(trace_transactions.call_contract.len(), 1);
        let parsed_tx = &trace_transactions.call_contract[0];
        assert_eq!(parsed_tx.transaction.hash, "aa5");
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::CallContract(_))
        ));

        assert_eq!(trace_transactions.gas_credit.len(), 1);
        let parsed_tx = &trace_transactions.gas_credit[0];
        assert_eq!(parsed_tx.transaction.hash, "aa6");
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::NativeGasPaid(_))
        ));
    }

    #[test]
    fn test_native_gas_added() {
        let traces = fixture_traces();

        let trace_transactions = TraceTransactions::from_trace(traces[5].clone()).unwrap();
        assert_eq!(trace_transactions.gas_added.len(), 1);
        let parsed_tx = &trace_transactions.gas_added[0];
        assert_eq!(
            parsed_tx.transaction.hash,
            "hlbJSt6b0kkNh0We16gyIxE5WyDRDltaKIYOmfEZtAs="
        );
        assert_eq!(
            parsed_tx.message_id,
            Some("0x0e6f759f68edb972cc1c5ac28ae44a026567c39d0a67d71de90978a12106a6ba".to_string())
        );
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::NativeGasAdded(_))
        ));
    }

    #[test]
    fn test_native_gas_refunded() {
        let traces = fixture_traces();

        let trace_transactions = TraceTransactions::from_trace(traces[7].clone()).unwrap();
        assert_eq!(trace_transactions.gas_refunded.len(), 1);
        let parsed_tx = &trace_transactions.gas_refunded[0];
        assert_eq!(
            parsed_tx.transaction.hash,
            "HbFekh+vKZKeNQkTBanWSiipy2/v0ynpQ4fiafFls3s="
        );
        assert_eq!(
            parsed_tx.message_id,
            Some("0xeb065d9d930349d0643b946d961ec600f80d5e5f30ab01df6f136243ee5035c2".to_string())
        );
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::NativeGasRefunded(_))
        ));
    }

    #[test]
    fn test_jetton_gas_paid() {
        let traces = fixture_traces();

        let trace_transactions = TraceTransactions::from_trace(traces[9].clone()).unwrap();
        assert_eq!(trace_transactions.gas_credit.len(), 1);
        let parsed_tx = &trace_transactions.gas_credit[0];
        assert_eq!(
            parsed_tx.transaction.hash,
            "/OxewvVQHSEhT6pz1L/et2BKJC7avRCYEx0FbUWPEuo="
        );
        assert_eq!(
            parsed_tx.message_id,
            Some("0xd59014fd585eed8bee519c40d93be23a991fdb7d68a41eb7ad678dc40510e65d".to_string())
        );
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::JettonGasPaid(_))
        ));
    }

    #[test]
    fn test_gas_credit_map_to_vec() {
        use crate::parse_trace::{
            gas_credit_map_to_vec, LogMessage, MessageMatchingKey, ParsedTransaction,
        };

        let traces = fixture_traces();

        // Create a fake CallContractMessage
        let call_contract_msg = CallContractMessage {
            destination_chain: "chain-A".to_string(),
            destination_address: "addr-123".to_string(),
            payload_hash: [1u8; 32],
            payload: "payload".to_string(),
            source_address: TonAddress::from_hex_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        };

        let call_contract_tx = traces[1].transactions[0].clone();

        let matching_key = MessageMatchingKey {
            destination_chain: call_contract_msg.destination_chain.clone(),
            destination_address: call_contract_msg.destination_address.clone(),
            payload_hash: call_contract_msg.payload_hash,
        };

        let native_gas_msg = NativeGasPaidMessage {
            destination_chain: call_contract_msg.destination_chain.clone(),
            destination_address: call_contract_msg.destination_address.clone(),
            payload_hash: call_contract_msg.payload_hash,
            _sender: TonAddress::from_hex_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            msg_value: BigUint::from(1u8),
            refund_address: TonAddress::from_hex_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
        };

        let gas_credit_tx = ParsedTransaction {
            transaction: call_contract_tx.clone(),
            log_message: Some(LogMessage::NativeGasPaid(native_gas_msg)),
            message_id: None,
        };

        let call_contract = vec![ParsedTransaction {
            transaction: call_contract_tx,
            log_message: Some(LogMessage::CallContract(call_contract_msg)),
            message_id: None,
        }];

        let mut gas_credit_map = HashMap::new();
        gas_credit_map.insert(matching_key, gas_credit_tx);

        let result = gas_credit_map_to_vec(&call_contract, gas_credit_map);

        assert_eq!(result.len(), 1);
        let matched = &result[0];
        assert_eq!(
            matched.transaction.hash,
            "HgpDv8z9uKb4vgtsFVGjyClfNcWzVJofXVk4+7FZHU4="
        );
        assert_eq!(
            matched.message_id.as_deref(),
            Some("0x1e0a43bfccfdb8a6f8be0b6c1551a3c8295f35c5b3549a1f5d5938fbb1591d4e")
        );
    }

    #[test]
    fn test_jetton_gas_added() {
        let traces = fixture_traces();

        let trace_transactions = TraceTransactions::from_trace(traces[10].clone()).unwrap();
        assert_eq!(trace_transactions.gas_added.len(), 1);
        let parsed_tx = &trace_transactions.gas_added[0];
        assert_eq!(
            parsed_tx.transaction.hash,
            "blzE/VLC5oz8yBYjKnSgUMomLj4oecIIiBwXZcxXY+k="
        );
        assert_eq!(
            parsed_tx.message_id,
            Some("0xb9ac1cbe75a96a7146a71df1bf5f3ac00668edba0b432d4c5fbe5d59162aced7".to_string())
        );
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::JettonGasAdded(_))
        ));
    }

    #[test]
    fn test_executed() {
        let traces = fixture_traces();

        let trace_transactions = TraceTransactions::from_trace(traces[11].clone()).unwrap();
        assert_eq!(trace_transactions.executed.len(), 1);
        let parsed_tx = &trace_transactions.executed[0];
        assert_eq!(
            parsed_tx.transaction.hash,
            "lx8e350/GIl5bTGULSGwnGhqoOZmndKjgW3aX7nkg+w="
        );
        assert!(matches!(
            parsed_tx.log_message,
            Some(LogMessage::Executed(_))
        ));
    }
}
