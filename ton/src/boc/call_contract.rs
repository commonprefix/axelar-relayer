/*!
Parses and represents a `CallContractMessage`.

# Usage Example

```rust,no_run
use ton::boc::call_contract::CallContractMessage;

let boc = "te6cck...";

match CallContractMessage::from_boc_b64(boc) {
    Ok(msg) => {
        // ...
    },
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

# See also

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/b1053bf982f21d6d207d30338f5b264505966948/contracts/axelar_gateway.fc#L672:L678

*/

use serde::{Deserialize, Serialize};
use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;
use crate::boc::cell_to::CellTo;
use crate::errors::BocError;
use crate::errors::BocError::{BocParsingError};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CallContractMessage {
    pub destination_chain: String,
    pub destination_address: String,
    pub payload: String,
    pub source_address: TonAddress,
    pub payload_hash: [u8; 32],
}

impl CallContractMessage {
    pub fn from_boc_b64(boc: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();

        let destination_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        let destination_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        let payload = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;

        let payload = hex::encode(payload);

        let source_address = parser.load_address().map_err(|err| BocParsingError(err.to_string()))?;

        let payload_hash: [u8; 32] = parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid payload hash length".to_string()))?;

        Ok(Self {
            destination_chain,
            destination_address,
            payload,
            source_address,
            payload_hash
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use primitive_types::H256;
    use tonlib_core::TonAddress;
    use crate::boc::call_contract::CallContractMessage;

    #[test]
    fn test_from_boc_b64() {
        let boc = "te6cckEBBAEA5QADg4AcPMZ9bgNiMWiFLuLZ3ODT3Qj2rbcRiS/f1NA9opZaWPXUykhs4AH2lBVEFjqex7VaPbPTvuLH5GEs5sIeXm+pcAECAwAcYXZhbGFuY2hlLWZ1amkAVDB4ZDcwNjdBZTNDMzU5ZTgzNzg5MGIyOEI3QkQwZDIwODRDZkRmNDliNQDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gUmVsYXllciEAAAAAAAAAAAAAAAAAne0F4Q==";
        let res = CallContractMessage::from_boc_b64(boc);
        let res = res.unwrap();
        assert_eq!(res.destination_chain, "avalanche-fuji");
        assert_eq!(res.destination_address, "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5");
        assert_eq!(res.source_address, TonAddress::from_str(&"EQDh5jPrcBsRi0QpdxbO5wae6Ee1bbiMSX7-poHtFLLSxyuC").unwrap());
        assert_eq!(res.payload, "0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001348656c6c6f2066726f6d2052656c617965722100000000000000000000000000");
        let payload_hash = format!("{:?}", H256::from(res.payload_hash));
        assert_eq!(payload_hash, "0xaea6524367000fb4a0aa20b1d4f63daad1ed9e9df7163f2309673610f2f37d4b");
    }
}
