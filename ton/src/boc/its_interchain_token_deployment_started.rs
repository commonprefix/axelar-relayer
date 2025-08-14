/*!

# Usage Example
```rust,no_run
use ton::boc::its_interchain_token_deployment_started::LogITSInterchainTokenDeploymentStartedMessage;

let boc_b64 = "b64 boc";

match LogITSInterchainTokenDeploymentStartedMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/d4b01c18fd2b4da73528aa5620b5fe23f71f84a9/contracts/interchain_token_service.fc#L452:L460

*/

use crate::boc::cell_to::CellTo;
use crate::boc::op_code::compare_op_code;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_INTERCHAIN_TOKEN_DEPLOYMENT_STARTED_LOG;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;

#[derive(Debug, Clone)]
pub struct LogITSInterchainTokenDeploymentStartedMessage {
    pub(crate) destination_chain: String,
    pub(crate) token_id: BigUint,
    pub(crate) token_name: String,
    pub(crate) token_symbol: String,
    pub(crate) decimals: u8,
}

impl LogITSInterchainTokenDeploymentStartedMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;

        if !compare_op_code(OP_INTERCHAIN_TOKEN_DEPLOYMENT_STARTED_LOG, &op_code) {
            return Err(InvalidOpCode(format!(
                "Expected {:08X}, got {}",
                OP_INTERCHAIN_TOKEN_DEPLOYMENT_STARTED_LOG,
                hex::encode(&op_code)
            )));
        }

        let token_id = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let token_name = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let token_symbol = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let decimals = parser
            .load_u8(8)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let _minter = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .parser()
            .load_address();
        let destination_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        Ok(Self {
            destination_chain,
            token_id,
            token_symbol,
            token_name,
            decimals,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::its_interchain_token_deployment_started::LogITSInterchainTokenDeploymentStartedMessage;
    use num_bigint::BigUint;
    use std::str::FromStr;

    #[test]
    fn test_from_boc_b64() {
        let response = LogITSInterchainTokenDeploymentStartedMessage::from_boc_b64(
            "te6cckEBBQEAUgAESgAAAQeoP4SReC9O3TOBA3OmvJWkL/SkYDgdXuT4b/M/ry37vAkBAgMEABRUZXN0IHRva2VuAA5UT05URVNUAAAAHGF2YWxhbmNoZS1mdWppJ0b+vg==",
        );
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );
        let log = response.expect("Failed to unwrap log");
        assert_eq!(log.destination_chain, "avalanche-fuji");
        assert_eq!(
            log.token_id,
            BigUint::from_str(
                "76100784879436770959190113963535317215282835248950831905149195120573357620156"
            )
            .expect("failed to decode")
        );

        assert_eq!(log.token_symbol, "TONTEST");
        assert_eq!(log.token_name, "Test token");
        assert_eq!(log.decimals, 9);
    }
}
