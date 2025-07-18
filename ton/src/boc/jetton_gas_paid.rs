/*! Ton Cross Chain Message: A parsed representation of a TON cross chain message.

# Usage Example
```rust,no_run
use ton::boc::jetton_gas_paid::JettonGasPaidMessage;

let boc_b64 = "b64 boc";

match JettonGasPaidMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse JettonGasPaid: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/e0573ff258a1da0977a7b4e805f7a0a2bb1b76f2/contracts/gas_service.fc#L103:111

*/

use crate::boc::cell_to::CellTo;
use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

#[derive(Debug, Clone)]
pub struct JettonGasPaidMessage {
    pub(crate) minter: TonAddress,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) amount: BigUint,
    pub(crate) refund_address: TonAddress,
    pub(crate) destination_chain: String,
    pub(crate) destination_address: String,
}

impl JettonGasPaidMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let minter = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let amount = parser
            .load_coins()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let payload_hash: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid payload hash length".to_string()))?;

        let refund_address = parser.next_reference().unwrap();
        let mut inner_parser = refund_address.parser();
        let refund_address = inner_parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let destination_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        let destination_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        Ok(Self {
            minter,
            payload_hash,
            amount,
            refund_address,
            destination_chain,
            destination_address,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::jetton_gas_paid::JettonGasPaidMessage;
    use num_bigint::BigUint;
    use primitive_types::H256;
    use tonlib_core::TonAddress;

    #[test]
    fn test_from_boc_b64() {
        let response = JettonGasPaidMessage::from_boc_b64("te6cckEBBAEAuQAEiYADLFxuu57x8vEB032J7Hw1/NB2lXuxCrpswCmAv+IsHSYehIFdTKSGzgAfaUFUQWOp7HtVo9s9O+4sfkYSzmwh5eb6lwMBAgMACHRvbjIAhDA6ZWQyMmRmMzQyMTlhZTI2MDM5ZmQ5NzdkOGU0MTlhZTE0ZDc4YjE5MmU5ZGI1ZGNmYTM1OTc4OTkwOTY0NzBkMQBDgBw8xn1uA2IxaIUu4tnc4NPdCPattxGJL9/U0D2illpY8JdJDvc=");
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );

        let log = response.unwrap();

        let payload_hash = format!("{:?}", H256::from(log.payload_hash));
        assert_eq!(
            payload_hash,
            "0xaea6524367000fb4a0aa20b1d4f63daad1ed9e9df7163f2309673610f2f37d4b"
        );
        assert_eq!(
            log.refund_address,
            TonAddress::from_base64_url("EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC")
                .unwrap()
        );
        assert_eq!(
            log.minter,
            TonAddress::from_base64_url("EQAZYuN13PePl4gOm-xPY-Gv5oO0q92IVdNmAUwF_xFg6fqe")
                .unwrap()
        );
        assert_eq!(log.destination_chain, "ton2");
        assert_eq!(
            log.destination_address,
            "0:ed22df34219ae26039fd977d8e419ae14d78b192e9db5dcfa3597899096470d1"
        );

        assert_eq!(log.amount, BigUint::from(1000000u32));
    }
}
