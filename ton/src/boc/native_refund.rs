/*!

# Usage Example

```rust,no_run
use num_bigint::BigUint;
use tonlib_core::{TonAddress, TonHash};
use ton::boc::native_refund::NativeRefundMessage;

let message_id = TonHash::from_hex("2b54edee081fc7cde3855988d4d28d948b99e132b062f6a724812713ec533215").unwrap();
let amount_to_refund = BigUint::from(10000000u32);
let destination_address: TonAddress = "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930".parse().unwrap();

let message = NativeRefundMessage::new(
    message_id,
    destination_address,
    amount_to_refund,
);

let ton_cell = message.to_cell().unwrap();
```

# TODO:
- Don't hardcode execute message gas

*/

use crate::error::BocError;
use crate::error::BocError::BocEncodingError;
use crate::ton_op_codes::OP_NATIVE_REFUND;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use tonlib_core::cell::{Cell, CellBuilder};
use tonlib_core::{TonAddress, TonHash};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NativeRefundMessage {
    tx_hash: TonHash,
    address: TonAddress,
    amount: BigUint,
}

impl NativeRefundMessage {
    pub fn new(tx_hash: TonHash, address: TonAddress, amount: BigUint) -> Self {
        Self {
            tx_hash,
            address,
            amount,
        }
    }

    pub fn to_cell(&self) -> Result<Cell, BocError> {
        let mut builder = CellBuilder::new();
        builder
            .store_uint(32, &BigUint::from(OP_NATIVE_REFUND))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        builder
            .store_tonhash(&self.tx_hash)
            .map_err(|e| BocEncodingError(e.to_string()))?;
        builder
            .store_address(&self.address)
            .map_err(|e| BocEncodingError(e.to_string()))?;
        builder
            .store_coins(&self.amount)
            .map_err(|e| BocEncodingError(e.to_string()))?;
        builder.build().map_err(|e| BocEncodingError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigUint;
    use std::str::FromStr;
    use tonlib_core::tlb_types::tlb::TLB;
    use tonlib_core::{TonAddress, TonHash};

    #[test]
    fn test_to_cell() {
        let address: TonAddress = "EQBGhqLAZseEqRXz4ByFPTGV7SVMlI4hrbs-Sps_Xzx01x8G"
            .parse()
            .unwrap();

        let tx_hash =
            TonHash::from_hex("2b54edee081fc7cde3855988d4d28d948b99e132b062f6a724812713ec533215")
                .unwrap();
        let amount = BigUint::from_str("4900000000").unwrap();
        let message = super::NativeRefundMessage::new(tx_hash, address, amount);

        let res = message.to_cell().unwrap();
        assert_eq!(res.to_boc_b64(true).unwrap(), "te6cckEBAQEATQAAlQAAAEYrVO3uCB/HzeOFWYjU0o2Ui5nhMrBi9qckgScT7FMyFYAI0NRYDNjwlSK+fAOQp6YyvaSpkpHENbdnyVNn6+eOmuoCSCAiAVsrXmA=");
    }
}
