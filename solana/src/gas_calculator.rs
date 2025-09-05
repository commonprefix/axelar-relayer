// use std::collections::HashMap;

// use crate::error::GasError;
// use solana_sdk::pubkey::Pubkey;
// use solana_types::solana_types::SolanaTransaction;

// #[derive(Clone)]
// pub struct GasCalculator {
//     our_addresses: Vec<Pubkey>,
// }

// impl GasCalculator {
//     pub fn new(our_addresses: Vec<Pubkey>) -> Self {
//         Self { our_addresses }
//     }

//     pub fn calc_message_gas(&self, tx: SolanaTransaction) -> Result<u64, GasError> {
//         let total = self.cost(tx)?;
//         Ok(if total > 0 { total as u64 } else { 0 })
//     }

//     fn cost(&self, tx: SolanaTransaction) -> Result<i128, GasError> {
//         let mut balances: HashMap<Pubkey, i128> = self
//             .our_addresses
//             .iter()
//             .cloned()
//             .map(|addr| (addr, 0))
//             .collect();

//         if is_our_transaction(&mut balances, &tx) {
//             add_cost(&mut balances, tx.account.clone(), tx.total_fees as i128);
//         }
//         for msg in tx.out_msgs.clone() {
//             if is_our_transaction(&mut balances, &tx) {
//                 add_cost(&mut balances, tx.account.clone(), extract_fwd_fee(&msg));
//             }
//             if let Some(dest) = msg.destination.clone() {
//                 let value = extract_msg_value(msg);
//                 // Us sending to someone
//                 if us_sending(&mut balances, &tx, &dest) {
//                     add_cost(&mut balances, tx.account.clone(), value);
//                 }
//                 if us_receiving(&mut balances, &tx, &dest) {
//                     add_cost(&mut balances, dest.clone(), 0 - value);
//                 }
//             }
//         }

//         let total: i128 = balances.values().cloned().sum();
//         Ok(total)
//     }
// }
