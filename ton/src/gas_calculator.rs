/*!

# TODO:

Document how we calculate gas and why is native gas refunded different
*/

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

    pub fn calc_message_gas_naive(&self, transactions: &[Transaction]) -> Result<u64, GasError> {
        let mut debit_total: u64 = 0;
        let mut credit_total: u64 = 0;

        for tx in transactions {
            for msg in &tx.out_msgs {
                let value: u64 = match &msg.value {
                    Some(v) => v
                        .parse()
                        .map_err(|_| GasError::GasCalculationError(v.clone()))?,
                    None => continue,
                };

                if self.load_address(&msg.destination)?.is_some() {
                    debit_total += value;
                }

                if self.load_address(&msg.source)?.is_some() {
                    credit_total += value;
                }
            }
        }

        Ok(credit_total.saturating_sub(debit_total))
    }

    pub fn calc_message_gas_detailed(&self, transactions: &[Transaction]) -> Result<u64, GasError> {
        let mut total: u64 = 0;

        for tx in transactions {
            if self.load_address(&Some(tx.account.clone()))?.is_some() {
                total = total + tx.total_fees
            }
        }

        Ok(total)
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
    fn test_calc_message_gas() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap(),
            TonAddress::from_base64_url("EQBcfOiB4SF73vEFm1icuf3oqaFHj1bNQgxvwHKkxAiIjxLZ")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas_naive(&traces[6].transactions);
        assert_eq!(amount.unwrap(), 159987200);
    }

    #[test]
    fn test_gas_detailed() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("EQCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4DlThU")
                .unwrap(),
            TonAddress::from_base64_url("kQCEKDERj88xS-gD7non_TITN-50i4QI8lMukNkqknAX28OJ")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas_detailed(&traces[7].transactions);
        assert_eq!(amount.unwrap(), 9453425);
    }

}
