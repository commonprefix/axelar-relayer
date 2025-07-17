/*!

Why we see empty balances?

1. **Every** transaction _always_ gets its `AccountStateBefore/After` fields primed with just the hash.
2. The code then does a lookup in `account_states` to fill in balance, status, etc—but only for those hashes that
_actually exist_ in the table.
3. If a particular state‐hash was never recorded in `account_states` (for example, the indexer didn’t snapshot every
single intermediate state, or that account had no on‐chain state change so no row was inserted), **no** row is returned
for that hash.

4. In that case the `AccountStateBefore/After` pointer remains the bare struct with only the hash, and all the other
fields (including `balance`) stay nil → you get an empty JSON object (no `balance`) for those slots.

# TODO:

Document how we calculate gas and why is native gas refunded different
*/

use std::collections::HashMap;
use std::ops::AddAssign;
use crate::error::GasError;
use relayer_base::ton_types::Transaction;
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

    fn load_address(&self, addr_str: &Option<String>) -> Result<Option<TonAddress>, GasError> {
        if let Some(s) = addr_str {
            let addr =
                TonAddress::from_str(s).map_err(|_| GasError::GasCalculationError(s.clone()))?;
            Ok(Some(addr).filter(|a| self.our_addresses.contains(a)))
        } else {
            Ok(None)
        }
    }

    pub fn calc_message_gas_native_gas_refunded(&self, transactions: &[Transaction]) -> Result<u64, GasError> {
        if transactions.len() < 3 {
            return Ok(0);
        }

        let balance_diff = self.balance_diff(transactions)?;
        let refund = i128::from_str(&transactions[2].out_msgs[0].value.clone().unwrap_or("0".to_string())).unwrap_or(0);
        let total = balance_diff - refund;

        Ok(if total > 0 {
            total as u64
        } else {
            0
        })
    }

    fn balance_diff(&self, transactions: &[Transaction]) -> Result<i128, GasError> {
        // We may lose some precision, but the computation should be much cheaper than using BigUInt
        let mut balances: HashMap<TonAddress, i128> = HashMap::new();

        for tx in transactions {

            let balance_before = i128::from_str(&tx.account_state_before.balance.clone().unwrap_or("0".to_string())).unwrap_or(0);
            let balance_after = i128::from_str(&tx.account_state_after.balance.clone().unwrap_or("0".to_string())).unwrap_or(0);
            let balance = balance_before - balance_after;
            if let Some(addr) = self.load_address(&Some(tx.account.clone()))? {
                balances.entry(addr.clone()).or_insert(0).add_assign(balance);
            }
        };

        let total: i128 = balances.values().cloned().sum();
        Ok(total)
    }

    pub fn calc_message_gas(&self, transactions: &[Transaction]) -> Result<u64, GasError> {
        let total = self.balance_diff(transactions)?;
        Ok(if total > 0 {
            total as u64
        } else {
            0
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::gas_calculator::GasCalculator;
    use relayer_base::ton_types::{Trace, TracesResponse, TracesResponseRest};
    use std::fs;
    use tonlib_core::TonAddress;

    fn fixture_traces() -> Vec<Trace> {
        let file_path = "tests/data/v3_traces.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        let rest: TracesResponseRest =
            serde_json::from_str(&body).expect("Failed to deserialize test transaction data");

        TracesResponse::from(rest).traces
    }

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
