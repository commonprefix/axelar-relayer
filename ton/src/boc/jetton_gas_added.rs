/*! Ton Cross Chain Message: A parsed representation of a TON cross chain message.

# Usage Example
```rust,no_run
use ton::boc::jetton_gas_added::JettonGasAddedMessage;

let boc_b64 = "b64 boc";

match JettonGasAddedMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse JettonGasAdded: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/e0573ff258a1da0977a7b4e805f7a0a2bb1b76f2/contracts/gas_service.fc#L289:295

*/

use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::{TonAddress, TonHash};

#[derive(Debug, Clone)]
pub struct JettonGasAddedMessage {
    pub(crate) minter: TonAddress,
    pub(crate) sender: TonAddress,
    pub(crate) tx_hash: TonHash,
    pub(crate) amount: BigUint,
    pub(crate) refund_address: TonAddress,
}

impl JettonGasAddedMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let minter = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let sender = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let amount = parser
            .load_coins()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let tx_hash: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid tx_hash hash length".to_string()))?;

        let tx_hash = TonHash::from(tx_hash);

        let refund_address = parser.next_reference().unwrap();
        let mut inner_parser = refund_address.parser();
        let refund_address = inner_parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        Ok(Self {
            minter,
            sender,
            tx_hash,
            amount,
            refund_address,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::jetton_gas_added::JettonGasAddedMessage;
    use num_bigint::BigUint;
    use tonlib_core::{TonAddress, TonHash};

    #[test]
    fn test_from_boc_b64() {
        let response = JettonGasAddedMessage::from_boc_b64("te6cckEBAgEAjwABz4ADLFxuu57x8vEB032J7Hw1/NB2lXuxCrpswCmAv+IsHTADh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7+poHtFLLSx0BfXhALmsHL51qWpxRqcd8b9fOsAGaO26C0MtTF++XVkWKs7XgAQBDgB2kW+aEM1xMBz+y77HIM1wprxYyXTtrufRrLxMhLI4aMM6GZMQ=");
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );

        let log = response.unwrap();

        assert_eq!(
            log.refund_address,
            TonAddress::from_base64_url("EQDtIt80IZriYDn9l32OQZrhTXixkunbXc-jWXiZCWRw0TXB")
                .unwrap()
        );
        assert_eq!(
            log.minter,
            TonAddress::from_base64_url("EQAZYuN13PePl4gOm-xPY-Gv5oO0q92IVdNmAUwF_xFg6fqe")
                .unwrap()
        );
        assert_eq!(
            log.sender,
            TonAddress::from_base64_url("EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC")
                .unwrap()
        );
        assert_eq!(
            log.tx_hash,
            TonHash::from_hex("b9ac1cbe75a96a7146a71df1bf5f3ac00668edba0b432d4c5fbe5d59162aced7")
                .unwrap()
        );

        assert_eq!(log.amount, BigUint::from(100000000u32));
    }
}
