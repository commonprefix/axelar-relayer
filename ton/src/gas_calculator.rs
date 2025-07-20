/*!

Why we see empty balances?

 * **Every** transaction _always_ gets its `AccountStateBefore/After` fields primed with just the hash.
 * The code then does a lookup in `account_states` to fill in balance, status, etc—but only for those hashes that _actually exist_ in the table.
 * If a particular state‐hash was never recorded in `account_states` (for example, the indexer didn’t snapshot every single intermediate state, or that account had no on‐chain state change so no row was inserted), **no** row is returned for that hash.
 * In that case the `AccountStateBefore/After` pointer remains the bare struct with only the hash, and all the other fields (including `balance`) stay nil → you get an empty JSON object (no `balance`) for those slots.

*/

use crate::error::GasError;
use ton_types::ton_types::Transaction;
use std::collections::HashMap;
use std::ops::AddAssign;
use std::str::FromStr;
use tonlib_core::TonAddress;

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
        // We may lose some precision, but the computation should be much cheaper than using BigUInt
        let mut balances: HashMap<TonAddress, i128> = HashMap::new();

        for tx in transactions {
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
            if let Some(addr) = self.load_address(&Some(tx.account.clone()))? {
                balances
                    .entry(addr.clone())
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
        assert_eq!(amount.unwrap(), 10869279);
    }
}
