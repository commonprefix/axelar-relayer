/*!

# Usage Example
```rust,no_run
use ton::boc::app_interchain_transfer_received::LogAppInterchainTransferReceived;

let boc_b64 = "b64 boc";

match LogAppInterchainTransferReceived::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/d4b01c18fd2b4da73528aa5620b5fe23f71f84a9/contracts/interchain_token_service.fc#L718:L726:

*/

use crate::boc::cell_to::CellTo;
use crate::boc::op_code::compare_op_code;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_INTERCHAIN_TRANSFER_RECEIVED_LOG;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

#[derive(Debug, Clone)]
pub struct LogAppInterchainTransferReceived {
    pub(crate) token_id: BigUint,
    pub(crate) source_chain: String,
    pub(crate) source_address: String,
    pub(crate) destination_address: TonAddress,
    pub(crate) amount: BigUint,
    pub(crate) data: Vec<u8>,
}

impl LogAppInterchainTransferReceived {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;

        if !compare_op_code(OP_INTERCHAIN_TRANSFER_RECEIVED_LOG, &op_code) {
            return Err(InvalidOpCode(format!(
                "Expected {:08X}, got {}",
                OP_INTERCHAIN_TRANSFER_RECEIVED_LOG,
                hex::encode(&op_code)
            )));
        }
        let token_id = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let source_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let source_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;
        let source_address = match std::str::from_utf8(&source_address) {
            Ok(s) => s.to_string(), // valid UTF-8 (so, valid ascii too), just use it as a string
            Err(_) => {
                let hex_str: String = source_address
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect();
                format!("0x{}", hex_str)
            }
        };

        let destination_address = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let amount = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .parser()
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;

        let data: Vec<u8> = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .data()
            .into();

        parser.ensure_empty().map_err(|err| BocParsingError(err.to_string()))?;

        Ok(Self {
            token_id,
            source_chain,
            source_address,
            destination_address,
            amount,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use num_bigint::BigUint;
    use tonlib_core::TonAddress;
    use crate::boc::app_interchain_transfer_received::LogAppInterchainTransferReceived;

    #[test]
    fn test_from_boc_b64() {
        let response = LogAppInterchainTransferReceived::from_boc_b64(
            "te6cckEBBQEAogAEiwAAAQY7aKPgEGHI4DOplpesxrI+eoKfjYFgNvZLEVdlNebutYAVf66zzgI92Xc1ALNkqEumRUKu0MrsT7K1c+WuvUzAXDABAgMEABxhdmFsYW5jaGUtZnVqaQBAEjRWeJASNFZ4kBI0VniQEjRWeJAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABkAADI0pyx",
        );
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );
        let log = response.expect("Failed to unwrap log");

        assert_eq!(log.amount, BigUint::from_str("100").unwrap());
        assert_eq!(log.source_chain, "avalanche-fuji");
        assert_eq!(log.source_address, "0x1234567890123456789012345678901234567890000000000000000000000000");
        assert_eq!(log.destination_address, TonAddress::from_str("0:abfd759e7011eecbb9a8059b25425d322a15768657627d95ab9f2d75ea6602e1").unwrap());
        assert_eq!(log.data.len(), 0);
    }

    #[test]
    fn test_from_boc_b64_sender_address_string() {
        let with_string = "te6cckEBBQEAwgAEiwAAAQY7aKPgEGHI4DOplpesxrI+eoKfjYFgNvZLEVdlNebutYAVf66zzgI92Xc1ALNkqEumRUKu0MrsT7K1c+WuvUzAXDABAgMEABxhdmFsYW5jaGUtZnVqaQCAMHgxMjM0NTY3ODkwMTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGQAAD3tUXk=";

        let response = LogAppInterchainTransferReceived::from_boc_b64(
            with_string
        );
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );
        let log = response.expect("Failed to unwrap log");

        assert_eq!(log.amount, BigUint::from_str("100").unwrap());
        assert_eq!(log.source_chain, "avalanche-fuji");
        assert_eq!(log.source_address, "0x1234567890123456789012345678901234567890\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
        assert_eq!(log.destination_address, TonAddress::from_str("0:abfd759e7011eecbb9a8059b25425d322a15768657627d95ab9f2d75ea6602e1").unwrap());
        assert_eq!(log.data.len(), 0);
    }
}
