/*!

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

        let refund_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
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
        let response = JettonGasPaidMessage::from_boc_b64("te6cckEBBAEAqQAEhYADLFxuu57x8vEB032J7Hw1/NB2lXuxCrpswCmAv+IsHSIDxrkMawLoqk/1cMqYanWtVhOn7os7RcEnPV9WakXMg80DAQIDABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AEOAHDzGfW4DYjFohS7i2dzg090I9q23EYkv39TQPaKWWljwjMXfiA==");
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );

        let log = response.unwrap();

        let payload_hash = format!("{:?}", H256::from(log.payload_hash));
        assert_eq!(
            payload_hash,
            "0xe35c863581745527fab8654c353ad6ab09d3f7459da2e0939eafab3522e641e6"
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
        assert_eq!(log.destination_chain, "avalanche-fuji");
        assert_eq!(
            log.destination_address,
            "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5"
        );

        assert_eq!(log.amount, BigUint::from(1u32));
    }
}
