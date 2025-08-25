/*!

# Usage Example
```rust,no_run
use ton::boc::its_interchain_transfer::LogITSInterchainTransferMessage;

let boc_b64 = "b64 boc";

match LogITSInterchainTransferMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/d4b01c18fd2b4da73528aa5620b5fe23f71f84a9/contracts/interchain_token_service.fc#L598:L606

*/

use crate::boc::cell_to::CellTo;
use crate::boc::op_code::compare_op_code;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_INTERCHAIN_TRANSFER_LOG;
use num_bigint::BigUint;
use std::str::FromStr;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

#[derive(Debug, Clone)]
pub struct LogITSInterchainTransferMessage {
    pub(crate) token_id: BigUint,
    pub(crate) sender_address: TonAddress,
    pub(crate) destination_chain: String,
    pub(crate) destination_address: String,
    pub(crate) jetton_amount: BigUint,
    pub(crate) data: Vec<u8>,
}

impl LogITSInterchainTransferMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;

        if !compare_op_code(OP_INTERCHAIN_TRANSFER_LOG, &op_code) {
            return Err(InvalidOpCode(format!(
                "Expected {:08X}, got {}",
                OP_INTERCHAIN_TRANSFER_LOG,
                hex::encode(&op_code)
            )));
        }

        let token_id = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;

        let sender_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .parser()
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let sender_address =
            TonAddress::from_str(&format!("0:{}", sender_address.to_str_radix(16)))
                .map_err(|err| BocParsingError(err.to_string()))?;

        let destination_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        let destination_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;

        let destination_address = format!("0x{}", hex::encode(&destination_address));

        let jetton_amount = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let data: Vec<u8> = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .data()
            .into();

        Ok(Self {
            token_id,
            sender_address,
            destination_chain,
            destination_address,
            jetton_amount,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::its_interchain_transfer::LogITSInterchainTransferMessage;
    use std::str::FromStr;
    use tonlib_core::TonAddress;

    #[test]
    fn test_from_boc_b64() {
        let response = LogITSInterchainTransferMessage::from_boc_b64(
            "te6cckEBBgEAlwAEiAAAARAGqiTxVdSBp9+zVVBJQeRlDVIDh8DkrUzNRniAwuoJUwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAQIDBABAH+D6DngoiSjKBZCMYX+5C9x+3eQp4UbKBIvc0xyJDnYAHGF2YWxhbmNoZS1mdWppAQAFAAAAKBI0VniQEjRWeJASNFZ4kBI0VniQPA51Og==",
        );
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );
        let log = response.expect("Failed to unwrap log");
        assert_eq!(
            log.token_id.to_str_radix(16),
            "6aa24f155d481a7dfb355504941e4650d520387c0e4ad4ccd467880c2ea0953"
        );
        assert_eq!(
            log.sender_address,
            TonAddress::from_str("EQAf4PoOeCiJKMoFkIxhf7kL3H7d5CnhRsoEi9zTHIkOdh-4").unwrap()
        );
        assert_eq!(
            log.destination_address,
            "0x1234567890123456789012345678901234567890"
        );
        assert_eq!(log.destination_chain, "avalanche-fuji");
        assert_eq!(log.data.len(), 0);
    }
}
