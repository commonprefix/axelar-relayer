/*! TonLog: A parsed representation of a TON log message.

# Usage Example
```rust,no_run
use ton::extract_log::TonLog;

let boc_b64 = "b64 boc";

match TonLog::from_boc_b64(boc_b64) {
    Ok(log) => {
         println!("Message ID: {}", log.message_id);
         println!("Source Chain: {}", log.source_chain);
         println!("Destination Chain: {}", log.destination_chain);
         // ... handle other fields
     }
     Err(e) => println!("Failed to parse TonLog: {:?}", e),
}
```

# TODO:

- Don't hardcode workchain
- Reuse error so it's BOC Parsing Error as type
*/

use crate::approve_message::ApproveMessagesError::BocParsingError;
use crate::approve_message::ApproveMessagesError;
use crate::cell_to::CellTo;
use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::{TonAddress, TonHash};

#[derive(Debug)]
pub struct TonLog {
    pub message_id: String,
    pub destination_address: String,
    pub destination_chain: String,
    pub source_address: String,
    pub source_chain: String,
    pub log_event: String
}

impl TonLog {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, ApproveMessagesError> {
        let cell = Cell::from_boc_b64(boc_b64).map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser = cell.parser();

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

        let hash_part: TonHash = destination_address.clone().try_into().map_err(|_| "Invalid hash length").unwrap();
        let ton_address = TonAddress::new(0, hash_part);

        let destination_chain = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        Ok(TonLog {
            message_id,
            source_chain,
            source_address,
            destination_chain,
            destination_address: ton_address.to_hex(),
            log_event: "".to_string(),
        })

    }
}


#[cfg(test)]
mod tests {
    use crate::extract_log::TonLog;

    #[test]
    fn test_ton_log() {
        let response = TonLog::from_boc_b64("te6cckEBBwEA1AAEAAECAwQAiDB4ZjM4ZDJhNjQ2ZTRiNjBlMzdiYzE2ZDU0YmI5MTYzNzM5MzcyNTk0ZGM5NmJhYjk1NGE4NWI0YTE3MGY0OWU1OC0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AkCeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwUGAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMs9py6Y=");
        assert!(response.is_ok(), "Failed to parse TonLog: {:?}", response.unwrap_err());

        let log = response.unwrap();
        assert_eq!(log.message_id, "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58-1");
        assert_eq!(log.source_chain, "avalanche-fuji");
        assert_eq!(log.source_address, "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5");
        assert_eq!(log.destination_chain, "ton2");
        assert_eq!(log.destination_address, "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930");
    }

    #[test]
    fn test_ton_log_invalid_boc() {
        let invalid_boc = "this_is_not_a_valid_boc_string";
        let response = TonLog::from_boc_b64(invalid_boc);
        assert!(response.is_err());
    }
}