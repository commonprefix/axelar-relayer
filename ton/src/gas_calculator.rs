/*!

Why we see empty balances?

 * **Every** transaction _always_ gets its `AccountStateBefore/After` fields primed with just the hash.
 * The code then does a lookup in `account_states` to fill in balance, status, etc—but only for those hashes that _actually exist_ in the table.
 * If a particular state‐hash was never recorded in `account_states` (for example, the indexer didn’t snapshot every single intermediate state, or that account had no on‐chain state change so no row was inserted), **no** row is returned for that hash.
 * In that case the `AccountStateBefore/After` pointer remains the bare struct with only the hash, and all the other fields (including `balance`) stay nil → you get an empty JSON object (no `balance`) for those slots.

*/

use crate::error::GasError;
use ton_types::ton_types::Transaction;
use std::collections::{HashMap, HashSet};
use std::ops::AddAssign;
use std::str::FromStr;
use tonlib_core::TonAddress;
use crate::ton_constants::{OP_MESSAGE_APPROVE, OP_NULLIFY_IF_APPROVED};

#[derive(Default)]
pub struct GasCalculator {
    our_addresses: Vec<TonAddress>,
}

impl GasCalculator {
    pub fn new(our_addresses: Vec<TonAddress>) -> Self {
        Self { our_addresses }
    }

    fn load_address(&self, addr: &Option<TonAddress>) -> Result<Option<TonAddress>, GasError> {
        if let Some(s) = addr {
            let addr = s.clone();
            Ok(Some(addr).filter(|a| self.our_addresses.contains(a)))
        } else {
            Ok(None)
        }
    }

    pub fn calc_message_gas_native_gas_refunded(
        &self,
        transactions: &[Transaction],
    ) -> Result<u64, GasError> {
        if transactions.len() < 3 || transactions[2].out_msgs.is_empty() {
            return Ok(0);
        }

        let balance_diff = self.balance_diff(transactions)?;
        let refund = i128::from_str(
            &transactions[2].out_msgs[0]
                .value
                .clone()
                .unwrap_or("0".to_string()),
        )
        .unwrap_or(0);
        let total = balance_diff - refund;

        Ok(if total > 0 { total as u64 } else { 0 })
    }

    fn balance_diff(&self, transactions: &[Transaction]) -> Result<i128, GasError> {
        let mut dynamic_addresses: HashSet<TonAddress> = self.our_addresses.iter().cloned().collect();
        let mut balances: HashMap<TonAddress, i128> = HashMap::new();

        for tx in transactions {
            if (self.load_address(&Some(tx.account.clone()))?).is_some() {
                for msg in &tx.out_msgs {
                    if let Some(op) = &msg.opcode {
                        if *op == OP_MESSAGE_APPROVE || *op == OP_NULLIFY_IF_APPROVED {
                            if let Some(dest) = &msg.destination {
                                dynamic_addresses.insert(dest.clone());
                            }
                        }
                    }
                }
            }

            let balance_before = i128::from_str(
                &tx.account_state_before
                    .balance
                    .clone()
                    .unwrap_or("0".to_string()),
            )
            .unwrap_or(0);
            let balance_after = i128::from_str(
                &tx.account_state_after
                    .balance
                    .clone()
                    .unwrap_or("0".to_string()),
            )
            .unwrap_or(0);
            let balance = balance_before - balance_after;
            if dynamic_addresses.contains(&tx.account) {
                balances
                    .entry(tx.account.clone())
                    .or_insert(0)
                    .add_assign(balance);
            }
        }

        let total: i128 = balances.values().cloned().sum();
        Ok(total)
    }

    pub fn calc_message_gas(&self, transactions: &[Transaction]) -> Result<u64, GasError> {
        let total = self.balance_diff(transactions)?;
        Ok(if total > 0 { total as u64 } else { 0 })
    }
}

#[cfg(test)]
mod tests {
    use crate::gas_calculator::GasCalculator;
    use crate::test_utils::fixtures::fixture_traces;
    use tonlib_core::TonAddress;

    #[test]
    fn test_gas_approved() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap(),
            TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas(&traces[2].transactions);
        assert_eq!(amount.unwrap(), 27244157);
    }

    #[test]
    fn test_gas_refund() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap(),
            TonAddress::from_base64_url("kQCEKDERj88xS-gD7non_TITN-50i4QI8lMukNkqknAX28OJ")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas_native_gas_refunded(&traces[7].transactions);
        assert_eq!(amount.unwrap(), 10908904);
    }

    #[test]
    fn test_gas_refund_regular() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap(),
            TonAddress::from_base64_url("kQCEKDERj88xS-gD7non_TITN-50i4QI8lMukNkqknAX28OJ")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas_native_gas_refunded(&traces[8].transactions);

        // We care about A + C network fees:
        // https://testnet.tonviewer.com/transaction/9e2300fdb67ef055031c1f1250c0ec76a5b0181692d41a383adb5df890faf5cc?section=valueFlow
        assert_eq!(amount.unwrap(), 10869279);
    }

    #[test]
    fn test_gas_approved_a() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b")
                .unwrap(),
            TonAddress::from_base64_url("kQAAGUqtjkIr7fQ_7nRtbZKdNp26slRopp1RNwbqaXi2OnXH")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas(&traces[12].transactions);
        assert_eq!(amount.unwrap(), 42744830);
    }

    #[test]
    fn test_gas_approved_b() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b")
                .unwrap(),
            TonAddress::from_base64_url("kQAAGUqtjkIr7fQ_7nRtbZKdNp26slRopp1RNwbqaXi2OnXH")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas(&traces[13].transactions);
        assert_eq!(amount.unwrap(), 42628574);
    }

    #[test]
    fn test_gas_executed() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b")
                .unwrap(),
            TonAddress::from_base64_url("kQAAGUqtjkIr7fQ_7nRtbZKdNp26slRopp1RNwbqaXi2OnXH")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas(&traces[11].transactions);

        // https://testnet.tonviewer.com/transaction/3594522d1f0b4ec694558538e1f4b701acae9f7c9ee282f5c73b08ca18cb322c?section=valueFlow
        // We should have the same number as A, B and C
        assert_eq!(amount.unwrap(), 36656408 + 4271633);
    }

}
