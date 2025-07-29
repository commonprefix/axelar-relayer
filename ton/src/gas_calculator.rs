use crate::error::GasError;
use std::collections::HashMap;
use std::ops::AddAssign;
use std::str::FromStr;
use ton_types::ton_types::{Transaction, TransactionMessage};
use tonlib_core::TonAddress;

#[derive(Default)]
pub struct GasCalculator {
    our_addresses: Vec<TonAddress>,
}

fn extract_fwd_fee(msg: &TransactionMessage) -> i128 {
    msg.fwd_fee
        .clone()
        .unwrap_or("0".to_string())
        .parse::<i128>()
        .unwrap_or(0)
}

fn add_cost(balances: &mut HashMap<TonAddress, i128>, account: TonAddress, cost: i128) {
    balances.entry(account).or_insert(0).add_assign(cost);
}

fn is_our_transaction(balances: &mut HashMap<TonAddress, i128>, tx: &&Transaction) -> bool {
    balances.contains_key(&tx.account)
}

fn us_receiving(
    balances: &mut HashMap<TonAddress, i128>,
    tx: &&Transaction,
    dest: &TonAddress,
) -> bool {
    !balances.contains_key(&tx.account) && balances.contains_key(dest)
}

fn us_sending(
    balances: &mut HashMap<TonAddress, i128>,
    tx: &&Transaction,
    dest: &TonAddress,
) -> bool {
    balances.contains_key(&tx.account) && !balances.contains_key(dest)
}

fn extract_msg_value(msg: TransactionMessage) -> i128 {
    msg.value
        .unwrap_or("0".to_string())
        .parse::<i128>()
        .unwrap_or(0)
}

impl GasCalculator {
    pub fn new(our_addresses: Vec<TonAddress>) -> Self {
        Self { our_addresses }
    }

    pub fn calc_message_gas_native_gas_refunded(
        &self,
        transactions: &[Transaction],
    ) -> Result<u64, GasError> {
        let tx2 = match transactions.get(2) {
            Some(tx) => tx,
            None => return Ok(0),
        };

        let out_msg = match tx2.out_msgs.first() {
            Some(msg) => msg,
            None => return Ok(0),
        };

        let refund = match &out_msg.value {
            Some(val_str) => i128::from_str(val_str).unwrap_or(0),
            None => 0,
        };

        let balance_diff = self.cost(transactions)?;
        let total = balance_diff - refund;

        Ok(if total > 0 { total as u64 } else { 0 })
    }

    fn cost(&self, transactions: &[Transaction]) -> Result<i128, GasError> {
        let mut balances: HashMap<TonAddress, i128> = self
            .our_addresses
            .iter()
            .cloned()
            .map(|addr| (addr, 0))
            .collect();

        for tx in transactions {
            if is_our_transaction(&mut balances, &tx) {
                add_cost(&mut balances, tx.account.clone(), tx.total_fees as i128);
            }
            for msg in tx.out_msgs.clone() {
                if is_our_transaction(&mut balances, &tx) {
                    add_cost(&mut balances, tx.account.clone(), extract_fwd_fee(&msg));
                }
                if let Some(dest) = msg.destination.clone() {
                    let value = extract_msg_value(msg);
                    // Us sending to someone
                    if us_sending(&mut balances, &tx, &dest) {
                        add_cost(&mut balances, tx.account.clone(), value);
                    }
                    if us_receiving(&mut balances, &tx, &dest) {
                        add_cost(&mut balances, dest.clone(), 0 - value);
                    }
                }
            }
        }

        let total: i128 = balances.values().cloned().sum();
        Ok(total)
    }

    pub fn calc_message_gas(&self, transactions: &[Transaction]) -> Result<u64, GasError> {
        let total = self.cost(transactions)?;
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
            TonAddress::from_base64_url("0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b")
                .unwrap(),
            TonAddress::from_base64_url("kQAAGUqtjkIr7fQ_7nRtbZKdNp26slRopp1RNwbqaXi2OnXH")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas(&traces[2].transactions);
        // We should equal: https://testnet.tonviewer.com/transaction/6c167d523f4a1ceecdcfacb477f39a6fb622e2174e7f70f4308009b6e51cf66f
        // Sum of network fees for A and B, plus sum of outgoing values from B to C, minus
        // incoming value from C to B
        assert_eq!(amount.unwrap(), 48524353);
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
        // https://testnet.tonviewer.com/transaction/943579cdd5d9a87ce149a2b08b019fdad1747fbc07c6058f0b3c8bfb239be80e?section=valueFlow
        assert_eq!(amount.unwrap(), 48724830);
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
        assert_eq!(amount.unwrap(), 48622974);
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
        // We should have the same as:
        // A networkfees + B network fees + B->C value - C->B value + B -> D value
        assert_eq!(amount.unwrap(), 40928034);
    }

    #[test]
    fn test_gas_approved_c() {
        let traces = fixture_traces();

        let our_addresses = vec![
            TonAddress::from_base64_url("0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b")
                .unwrap(),
            TonAddress::from_base64_url("kQBLABicwV3LJN07SVHHnC4HB1cAKkerrNODHzrzwZmsKqmO")
                .unwrap(),
        ];

        let calc = GasCalculator::new(our_addresses);
        let amount = calc.calc_message_gas(&traces[16].transactions);
        // https://testnet.tonviewer.com/transaction/64d36666fde95c4022ddda652db2047cade337ab9423496d727ab325d33fd230?section=valueFlow
        assert_eq!(amount.unwrap(), 48647601);
    }
}
