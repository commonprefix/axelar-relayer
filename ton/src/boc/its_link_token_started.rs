/*!

# Usage Example
```rust,no_run
use ton::boc::its_link_token_started::LogITSLinkTokenStartedMessage;

let boc_b64 = "b64 boc";

match LogITSLinkTokenStartedMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
        // Handle fields
    }
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/d4b01c18fd2b4da73528aa5620b5fe23f71f84a9/contracts/interchain_token_service.fc#L632:L639

*/

use crate::boc::cell_to::CellTo;
use crate::boc::op_code::compare_op_code;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_LINK_TOKEN_STARTED_LOG;
use num_bigint::BigUint;
use relayer_base::gmp_api::gmp_types::TokenManagerType;
use std::str::FromStr;
use tonlib_core::cell::Cell;
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

#[derive(Debug, Clone)]
pub struct LogITSLinkTokenStartedMessage {
    pub(crate) token_id: BigUint,
    pub(crate) destination_chain: String,
    pub(crate) source_token_address: TonAddress,
    pub(crate) destination_token_address: String,
    pub(crate) token_manager_type: TokenManagerType,
}

impl LogITSLinkTokenStartedMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;

        if !compare_op_code(OP_LINK_TOKEN_STARTED_LOG, &op_code) {
            return Err(InvalidOpCode(format!(
                "Expected {:08X}, got {}",
                OP_LINK_TOKEN_STARTED_LOG,
                hex::encode(&op_code)
            )));
        }

        let token_id = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let destination_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let source_token_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .parser()
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let source_token_address =
            TonAddress::from_str(&format!("0:{}", source_token_address.to_str_radix(16)))
                .map_err(|err| BocParsingError(err.to_string()))?;
        let destination_token_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;
        let destination_token_address = format!("0x{}", hex::encode(&destination_token_address));
        let token_manager_type = parser
            .load_u8(8)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let token_manager_type = match token_manager_type {
            0 => TokenManagerType::NativeInterchainToken,
            1 => TokenManagerType::MintBurnFrom,
            2 => TokenManagerType::LockUnlock,
            3 => TokenManagerType::LockUnlockFee,
            4 => TokenManagerType::MintBurn,
            other => {
                return Err(BocParsingError(format!(
                    "Invalid token manager type {other}"
                )));
            }
        };

        Ok(Self {
            token_id,
            destination_chain,
            source_token_address,
            destination_token_address,
            token_manager_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::its_link_token_started::LogITSLinkTokenStartedMessage;
    use num_bigint::BigUint;
    use relayer_base::gmp_api::gmp_types::TokenManagerType;
    use std::str::FromStr;
    use tonlib_core::TonAddress;

    #[test]
    fn test_from_boc_b64() {
        let response = LogITSLinkTokenStartedMessage::from_boc_b64(
            "te6cckEBBQEAdQAESgAAAQU5aO+Enn04pm2AGPEU51UMmn1nuKcaUkOfFXOz8LgyigQBAgMEABxhdmFsYW5jaGUtZnVqaQBA+DyL4Yz0ZcelnxPvEyWNQkVzHPnLINq2ie/m2TMUbjcAKBKHpvtekQHhBLrvQEwN7A0hIGuaAAB7CRi0",
        );
        assert!(
            response.is_ok(),
            "Failed to parse: {:?}",
            response.unwrap_err()
        );
        let log = response.expect("Failed to unwrap log");
        assert_eq!(
            log.token_id,
            BigUint::from_str(
                "25967237556763834250448421157752164845917487892249674121800212115404164182666"
            )
            .expect("Failed to parse BigUint")
        );
        assert_eq!(log.destination_chain, "avalanche-fuji");
        assert_eq!(
            log.source_token_address,
            TonAddress::from_str(
                "0:f83c8be18cf465c7a59f13ef13258d4245731cf9cb20dab689efe6d933146e37"
            )
            .expect("Failed to parse TonAddress from string")
        );
        assert_eq!(log.token_manager_type, TokenManagerType::MintBurn);
        assert_eq!(
            log.destination_token_address,
            "0x1287a6fb5e9101e104baef404c0dec0d21206b9a"
        );
    }
}
