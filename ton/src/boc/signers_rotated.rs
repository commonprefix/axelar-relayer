/*!

# Usage Example
```rust,no_run
use ton::boc::signers_rotated::LogSignersRotatedMessage;

let boc_b64 = "b64 boc";

match LogSignersRotatedMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse JettonGasPaid: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/e0573ff258a1da0977a7b4e805f7a0a2bb1b76f2/contracts/gas_service.fc#L103:111

*/

use crate::boc::op_code::compare_op_code;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_SIGNERS_ROTATED_LOG;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;

#[derive(Debug, Clone)]
pub struct LogSignersRotatedMessage {
    pub(crate) signers_hash: String,
    pub(crate) epoch: u64,
}

impl LogSignersRotatedMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();
        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;

        if !compare_op_code(OP_SIGNERS_ROTATED_LOG, &op_code) {
            return Err(InvalidOpCode(format!(
                "Expected {:08X}, got {}",
                OP_SIGNERS_ROTATED_LOG,
                hex::encode(&op_code)
            )));
        }

        let _ref = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let signers_hash = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let signers_hash = format!("0x{}", hex::encode(signers_hash.to_bytes_be()));

        let epoch = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let epoch = u64::try_from(epoch).map_err(|err| BocParsingError(err.to_string()))?;

        Ok(Self {
            signers_hash,
            epoch,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::signers_rotated::LogSignersRotatedMessage;

    #[test]
    fn test_from_boc_b64() {
        let response = LogSignersRotatedMessage::from_boc_b64("te6cckECCQEAAlYAAYgAAAAq7gRMy82jn+eK+EpdUOFEI2/fqhGKCB9LANUHfOrwQfUAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgEBYYAAAAAAAAAAAAAAAAAAAAGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAUACAgLOAwYCASAEBQDhAOhB7/zzhC+HXDdGOdLwJln5NYwm6UNXx3chmQSVTG4AAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAA4RDzcAj0i1fnhB9GgaTRX010dEOt9IcchGS9W9d5AZl0wAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAgEgBwgA4RIHt53LaFXhcnlmghiAbjD+cMHEp4/xtfzSqTf7VgU5QAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAOEebVWLo/mVPlAeLES6KmLp5AfhTrmlb7X4OORC60ElmQAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAoCf4Q=");
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );

        let log = response.unwrap();

        assert_eq!(
            log.signers_hash,
            "0xee044ccbcda39fe78af84a5d50e144236fdfaa118a081f4b00d5077ceaf041f5"
        );
        assert_eq!(log.epoch, 2u64);
    }
}
