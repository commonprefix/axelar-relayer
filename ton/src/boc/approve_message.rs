/*!
Approve Message decoder

# Usage Example

```rust,no_run
use crate::ton::boc::approve_message::ApproveMessages;

let boc = "1234abcd";
let approve_messages = ApproveMessages::from_boc_hex(boc);
```
*/

use crate::ton_constants::OP_APPROVE_MESSAGES;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tonlib_core::cell::dict::predefined_readers::{key_reader_u8, val_reader_ref_cell};
use tonlib_core::cell::{ArcCell, Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;
use crate::boc::cell_to::CellTo;
use crate::error::BocError;
use crate::error::BocError::{BocParsingError, InvalidOpCode};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ApproveMessage {
    pub(crate) message_id: String,
    pub(crate) source_chain: String,
    source_address: String,
    destination_chain: String,
    destination_address: Vec<u8>,
    pub(crate) payload_hash: BigUint,
}

#[derive(Debug)]
pub struct ApproveMessages {
    pub approve_messages: Vec<ApproveMessage>,
    _proof: ArcCell,
}

impl ApproveMessages {
    pub fn from_boc_hex(boc: &str) -> Result<Self, BocError> {
        let cell = Cell::from_boc_hex(boc).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();
        let op_code = parser
            .load_bits(32)
            .map_err(|err| BocParsingError(err.to_string()))?;
        if hex::encode(&op_code) != format!("{:08X}", OP_APPROVE_MESSAGES) {
            return Err(InvalidOpCode(format!(
                "Expected {:?}, got {:?}",
                OP_APPROVE_MESSAGES,
                hex::encode(op_code)
            )));
        }
        let proof = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let messages = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut messages_parser = messages.parser();
        let data = messages_parser
            .load_dict(16, key_reader_u8, val_reader_ref_cell)
            .map_err(|err| BocParsingError(err.to_string()))?;
        let approve_messages: Vec<ApproveMessage> = data
            .values()
            .map(|cell| {
                let arc_cell =
                    Arc::from_cell(cell).map_err(|err| BocParsingError(err.to_string()))?;
                Self::parse_approve_messages(arc_cell)
            })
            .collect::<Result<_, _>>()?;
        Ok(ApproveMessages {
            approve_messages,
            _proof: proof,
        })
    }

    fn parse_approve_messages(cell: ArcCell) -> Result<ApproveMessage, BocError> {
        let mut parser = cell.parser();

        let payload_hash = parser
            .load_uint(256)
            .map_err(|err| BocParsingError(err.to_string()))?;
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
        let destination_address = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;
        let destination_chain = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        Ok(ApproveMessage {
            message_id,
            source_chain,
            destination_chain,
            source_address,
            destination_address,
            payload_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::prelude::BASE64_STANDARD;
    use base64::Engine;
    use std::str::FromStr;

    #[test]
    fn test_decode_approve_message() {
        let approve_message = hex::encode(BASE64_STANDARD.decode("te6cckECDAEAAYsAAggAAAAoAQIBYYAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADf5gkADAQHABADi0LAAUYmshNOh1nWEdwB3eJHd51H6EH1kg3v2M30y32eQAAAAAAAAAAAAAAAAAAAAASGIs1u/0XOSucVrtdkUCyCdtFVEh1Lza1vipAJyhYbSywcY60rdBpjT3ZipVjftz2pHY0znhNP8+FqIdPGpWwsBAtAFBECeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwYHCAkAiDB4ZjA0MzFkYThhNzdiYmVhYWNiNTMzYWIxZmZkMmI5MzhlY2I1MWM1MzAyOTllNDU2ZTA5ZTczNzlkOTlhMmYxZS0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AgAKCwBAuHpKD2RLehhu5xoUVGNPcMIqYqyhprpna1F1wh1/2TAACHRvbjJujf/t").unwrap());
        let approve_messages = ApproveMessages::from_boc_hex(approve_message.as_str());
        assert!(approve_messages.is_ok());

        let res = approve_messages.unwrap();

        assert_eq!(res.approve_messages.len(), 1);
        assert_eq!(
            res.approve_messages[0].message_id,
            "0xf0431da8a77bbeaacb533ab1ffd2b938ecb51c530299e456e09e7379d99a2f1e-1".to_string()
        );
        assert_eq!(
            res.approve_messages[0].source_chain,
            "avalanche-fuji".to_string(),
        );
        assert_eq!(
            res.approve_messages[0].source_address,
            "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5".to_string()
        );
        assert_eq!(
            res.approve_messages[0].destination_chain,
            "ton2".to_string()
        );
        assert_eq!(
            res.approve_messages[0].destination_address,
            vec![
                184, 122, 74, 15, 100, 75, 122, 24, 110, 231, 26, 20, 84, 99, 79, 112, 194, 42, 98,
                172, 161, 166, 186, 103, 107, 81, 117, 194, 29, 127, 217, 48,
            ],
        );
        assert_eq!(
            res.approve_messages[0].payload_hash,
            BigUint::from_str(
                "71468550630404048420691790219403539000788302635511547374558478410759778184983"
            )
            .unwrap()
        );
    }

    #[test]
    fn test_decode_malformed_message() {
        let approve_message = hex::encode(BASE64_STANDARD.decode("abccckECDAEAAYsAAggAAAAoAQIBYYAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADf5gkADAQHABADi0LAAUYmshNOh1nWEdwB3eJHd51H6EH1kg3v2M30y32eQAAAAAAAAAAAAAAAAAAAAAQ+j+g0KWjWTaPqB9qQHuWZQn7IPz7x3xzwbprT1a85sjh0UlPlFU84LDdRcD4GZ6n6GJlEKKTlRW5QtlzKGrAsBAtAFBECeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwYHCAkAiDB4MTdmZDdkYTNkODE5Y2ZiYzQ2ZmYyOGYzZDgwOTgwNzcwZWMxYjgwZmQ3ZDFiMjI5Y2VjMzI1MTkzOWI5YjIzZi0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AgAKCwBAuHpKD2RLehhu5xoUVGNPcMIqYqyhprpna1F1wh1/2TAACHRvbjJLddsV").unwrap());
        let approve_messages = ApproveMessages::from_boc_hex(approve_message.as_str());
        assert!(approve_messages.is_err());
        approve_messages.expect_err("Bag of cells deserialization error (BoC deserialization error: Unsupported cell magic number: 1773608050)");
    }
}
