/*!

# Usage Example
```rust,no_run
use ton::boc::its_token_metadata_registered::LogTokenMetadataRegisteredMessage;

let boc_b64 = "b64 boc";

match LogTokenMetadataRegisteredMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse JettonGasAdded: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/d4b01c18fd2b4da73528aa5620b5fe23f71f84a9/contracts/interchain_token_service.fc#L1215:L1219

*/

use crate::boc::op_code::compare_op_code;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_TOKEN_METADATA_REGISTERED_LOG;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

#[derive(Debug, Clone)]
pub struct LogTokenMetadataRegisteredMessage {
    pub(crate) address: TonAddress,
    pub(crate) decimals: u8,
}

impl LogTokenMetadataRegisteredMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;

        if !compare_op_code(OP_TOKEN_METADATA_REGISTERED_LOG, &op_code) {
            return Err(InvalidOpCode(format!(
                "Expected {:08X}, got {}",
                OP_TOKEN_METADATA_REGISTERED_LOG,
                hex::encode(&op_code)
            )));
        }

        let address = parser
            .load_address()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let decimals = parser
            .load_i16(8)
            .map_err(|err| BocParsingError(err.to_string()))? as u8;

        Ok(Self { address, decimals })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::its_token_metadata_registered::LogTokenMetadataRegisteredMessage;
    use tonlib_core::TonAddress;

    #[test]
    fn test_from_boc_b64() {
        let response = LogTokenMetadataRegisteredMessage::from_boc_b64(
            "te6cckEBAQEAKQAATQAAAQSAE8Gv5Obs3vWv/nwXHPbEvYTP0XH+WhNCBqP2z31FEZQhMELrLwg=",
        );
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );
        let log = response.expect("Failed to unwrap log");
        assert_eq!(log.decimals, 9);
        assert_eq!(
            log.address,
            TonAddress::from_base64_url("EQCeDX8nN2b3rX_z4LjntiXsJn6Lj_LQmhA1H7Z76iiMoe62")
                .expect("Failed to load address")
        );
    }
}
