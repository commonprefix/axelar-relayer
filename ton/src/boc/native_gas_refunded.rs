/*! Ton Cross Chain Message: A parsed representation of a TON cross chain message.

# Usage Example
```rust,no_run
use ton::boc::native_gas_refunded::NativeGasRefundedMessage;

let boc_b64 = "b64 boc";

match NativeGasRefundedMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse NativeGasRefunded: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/b1053bf982f21d6d207d30338f5b264505966948/contracts/gas_service.fc#L410

*/

use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::{TonAddress, TonHash};

#[derive(Debug, Clone)]
pub struct NativeGasRefundedMessage {
    pub(crate) tx_hash: TonHash,
    pub(crate) address: TonAddress,
    pub(crate) amount: BigUint,
}

impl NativeGasRefundedMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();
        let tx_hash = parser.load_tonhash().map_err(|err| BocParsingError(err.to_string()))?;
        let address = parser.load_address().map_err(|err| BocParsingError(err.to_string()))?;
        let amount = parser.load_coins().map_err(|err| BocParsingError(err.to_string()))?;
        Ok(Self {
            tx_hash,
            address,
            amount
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use num_bigint::BigUint;
    use tonlib_core::{TonAddress, TonHash};
    use crate::boc::native_gas_refunded::NativeGasRefundedMessage;

    #[test]
    fn test_from_boc_b64() {
        let response = NativeGasRefundedMessage::from_boc_b64("te6cckEBAQEASAAAi+sGXZ2TA0nQZDuUbZYexgD4DV5fMKsB328TYkPuUDXCgBw8xn1uA2IxaIUu4tnc4NPdCPattxGJL9/U0D2illpY6B/RX5+w1u5M").unwrap();

        assert_eq!(response.address, TonAddress::from_str("EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC").unwrap());
        assert_eq!(response.amount, BigUint::from(266907599u64));
        assert_eq!(response.tx_hash, TonHash::from_hex("eb065d9d930349d0643b946d961ec600f80d5e5f30ab01df6f136243ee5035c2").unwrap());
    }
}
