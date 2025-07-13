/*!
Parses and represents a `AddNativeGasMessage`.

# Usage Example

```rust,no_run
use ton::boc_native_gas_added::NativeGasAddedMessage;

let boc = "te6cck...";

match NativeGasAddedMessage::from_boc_b64(boc) {
    Ok(msg) => {
        // ...
    },
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/b1053bf982f21d6d207d30338f5b264505966948/contracts/gas_service.fc#L255:L259

*/

use crate::errors::BocError;
use crate::errors::BocError::BocParsingError;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::{TonAddress, TonHash};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NativeGasAddedMessage {
    pub tx_hash: TonHash,
    pub refund_address: TonAddress,
    pub msg_value: BigUint,
}

impl NativeGasAddedMessage {
    pub fn from_boc_b64(boc: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();

        let tx_hash: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid tx_hash hash length".to_string()))?;

        let tx_hash = TonHash::from(tx_hash);

        let refund_address = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let msg_value: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid msg_value hash length".to_string()))?;
        let msg_value = BigUint::from_bytes_be(&msg_value);

        Ok(Self {
            tx_hash,
            refund_address,
            msg_value,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc_native_gas_added::NativeGasAddedMessage;
    use num_bigint::BigUint;
    use std::str::FromStr;
    use tonlib_core::{TonAddress, TonHash};

    #[test]
    fn test_from_boc_b64() {
        let boc = "te6cckEBAQEAZAAAww5vdZ9o7blyzBxaworkSgJlZ8OdCmfXHekJeKEhBqa6gBw8xn1uA2IxaIUu4tnc4NPdCPattxGJL9/U0D2illpY4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAI68SIQfPwIyg==";
        let res = NativeGasAddedMessage::from_boc_b64(boc);
        let res = res.unwrap();
        assert_eq!(
            res.tx_hash,
            TonHash::from_hex("0e6f759f68edb972cc1c5ac28ae44a026567c39d0a67d71de90978a12106a6ba")
            .unwrap()
        );
        assert_eq!(
            res.refund_address,
            TonAddress::from_str(
                &"0:e1e633eb701b118b44297716cee7069ee847b56db88c497efea681ed14b2d2c7"
            )
            .unwrap()
        );
        assert_eq!(res.msg_value, BigUint::from(299338000u32));
    }
}
