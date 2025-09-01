use crate::error::GasError;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone)]
pub struct GasCalculator {
    our_addresses: Vec<Pubkey>,
}

impl GasCalculator {
    pub fn new(our_addresses: Vec<Pubkey>) -> Self {
        Self { our_addresses }
    }
}
