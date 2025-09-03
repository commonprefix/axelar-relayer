#[derive(Eq, Hash, PartialEq)]
pub struct MessageMatchingKey {
    pub(crate) destination_chain: String,
    pub(crate) destination_address: String,
    pub(crate) payload_hash: [u8; 32],
}
