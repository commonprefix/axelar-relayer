/*!
Parses and represents a `NullifiedSuccessfullyMessage`.


# Usage Example

```rust,no_run
use ton::boc::nullified_message::NullifiedSuccessfullyMessage;

let boc = "te6cck...";

match NullifiedSuccessfullyMessage::from_boc_b64(boc) {
    Ok(msg) => {
        // println!("Message ID: {}", msg.message_id);
        // println!("Source Chain: {}", msg.source_chain);
        // println!("Payload (hex): {}", msg.payload);
    },
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

*/

use crate::boc::cell_to::CellTo;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};
use crate::ton_constants::OP_NULLIFIED_SUCCESSFULLY;
use serde::{Deserialize, Serialize};
use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NullifiedSuccessfullyMessage {
    pub(crate) message_id: String,
    pub(crate) source_chain: String,
    pub(crate) source_address: String,
    destination_chain: String,
    destination_address: Vec<u8>,
    payload: String,
}

impl NullifiedSuccessfullyMessage {
    pub fn from_boc_b64(boc: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_b64(boc).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();

        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;
        if hex::encode(&op_code) != format!("{:08X}", OP_NULLIFIED_SUCCESSFULLY) {
            return Err(InvalidOpCode(format!(
                "Expected {:?}, got {:?}",
                OP_NULLIFIED_SUCCESSFULLY,
                hex::encode(op_code)
            )));
        }

        let message_id = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let source_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let source_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        let inner_cell = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut inner_parser: CellParser = inner_cell.parser();
        let payload = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;
        let payload = hex::encode(payload);
        let destination_address = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;
        let destination_chain = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        Ok(Self {
            message_id,
            source_chain,
            source_address,
            destination_chain,
            destination_address,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::nullified_message::NullifiedSuccessfullyMessage;

    #[test]
    fn test_from_boc_b64() {
        let boc = "te6cckECCAEAAV0ABIsAAAAFgBIHqwhg5lg4ES2+GWhwn4EVgGvmj7MoTr6OJXwhB8Byr9KMj8CFtEqwFmUtJVgVpEqk3ftJTCRWAx2zya/xlWvwAQIDBACIMHhmMmI3NDFmYjBiMmMyZmNmOTJhY2E4MjM5NWJjNjVkYWI0ZGQ4MjM5YTEyZjM2NmQ2MDQ1NzU1ZTBiMDJjMmEyLTEAHGF2YWxhbmNoZS1mdWppAFQweGQ3MDY3QWUzQzM1OWU4Mzc4OTBiMjhCN0JEMGQyMDg0Q2ZEZjQ5YjUDAAUGBwDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD0hlbGxvIGZyb20gdG9uIQAAAAAAAAAAAAAAAAAAAAAAAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMtlN//0=";
        let res = NullifiedSuccessfullyMessage::from_boc_b64(boc);
        let res = res.unwrap();
        assert_eq!(res.payload, "0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000f48656c6c6f2066726f6d20746f6e210000000000000000000000000000000000");
        assert_eq!(
            res.message_id,
            "0xf2b741fb0b2c2fcf92aca82395bc65dab4dd8239a12f366d6045755e0b02c2a2-1"
        );
        assert_eq!(res.source_chain, "avalanche-fuji");
        assert_eq!(
            res.source_address,
            "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5"
        );
        assert_eq!(res.destination_chain, "ton2");
        assert_eq!(
            res.destination_address,
            [
                184, 122, 74, 15, 100, 75, 122, 24, 110, 231, 26, 20, 84, 99, 79, 112, 194, 42, 98,
                172, 161, 166, 186, 103, 107, 81, 117, 194, 29, 127, 217, 48
            ]
            .to_vec()
        );
    }
}
