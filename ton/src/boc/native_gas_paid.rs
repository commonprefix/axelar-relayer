/*! Ton Cross Chain Message: A parsed representation of a TON cross chain message.

# Usage Example
```rust,no_run
use ton::boc::native_gas_paid::NativeGasPaidMessage;

let boc_b64 = "b64 boc";

match NativeGasPaidMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse NativeGasPaid: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/b1053bf982f21d6d207d30338f5b264505966948/contracts/gas_service.fc#L59:L65

*/

use crate::boc::cell_to::CellTo;
use crate::errors::BocError;
use crate::errors::BocError::BocParsingError;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

#[derive(Debug, Clone)]
pub struct NativeGasPaidMessage {
    pub(crate) sender: TonAddress,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) msg_value: BigUint,
    pub(crate) _refund_address: TonAddress,
    pub(crate) destination_chain: String,
    pub(crate) destination_address: String,
}

impl NativeGasPaidMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let sender = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let payload_hash: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid payload hash length".to_string()))?;

        let msg_value: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid msg_value hash length".to_string()))?;
        
        let msg_value = BigUint::from_bytes_be(&msg_value);

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
            sender,
            payload_hash,
            msg_value,
            _refund_address: refund_address,
            destination_chain,
            destination_address,
        })
    }
}

#[cfg(test)]
mod tests {
    use primitive_types::H256;
    use tonlib_core::TonAddress;
    use crate::boc::native_gas_paid::NativeGasPaidMessage;

    #[test]
    fn test_from_boc_b64() {
        let response = NativeGasPaidMessage::from_boc_b64("te6cckEBBAEAxwADw4AcPMZ9bgNiMWiFLuLZ3ODT3Qj2rbcRiS/f1NA9opZaWPXUykhs4AH2lBVEFjqex7VaPbPTvuLH5GEs5sIeXm+pYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA3BVoQAQIDAEOAHDzGfW4DYjFohS7i2dzg090I9q23EYkv39TQPaKWWljwABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1+B8bUw==");
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
            log._refund_address,
            TonAddress::from_base64_url("EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC")
                .unwrap()
        );
        assert_eq!(
            log.sender,
            TonAddress::from_base64_url("EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC")
                .unwrap()
        );
        assert_eq!(log.destination_chain, "avalanche-fuji");
        assert_eq!(
            log.destination_address,
            "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5"
        );
    }
}
