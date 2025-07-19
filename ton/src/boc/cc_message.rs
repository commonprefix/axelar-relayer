/*! Ton Cross Chain Message: A parsed representation of a TON cross chain message.

# Usage Example
```rust,no_run
use ton::boc::cc_message::TonCCMessage;

let boc_b64 = "b64 boc";

match TonCCMessage::from_boc_b64(boc_b64) {
    Ok(log) => {
         println!("Message ID: {}", log.message_id);
         println!("Source Chain: {}", log.source_chain);
         println!("Destination Chain: {}", log.destination_chain);
         // ... handle other fields
     }
     Err(e) => println!("Failed to parse TonLog: {:?}", e),
}
```

# See also:

- https://github.com/commonprefix/axelar-gmp-sdk-ton/blob/b1053bf982f21d6d207d30338f5b264505966948/contracts/axelar_gateway.fc#L528:L543

*/

use crate::boc::cell_to::CellTo;
use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::{TonAddress, TonHash};
use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use crate::ton_constants::WORKCHAIN;

#[derive(Debug, Clone)]
pub struct TonCCMessage {
    pub message_id: String,
    pub destination_address: String,
    pub destination_chain: String,
    pub source_address: String,
    pub source_chain: String,
    pub log_event: String,
    pub payload_hash: [u8; 32],
}

impl TonCCMessage {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, BocError> {
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

        let payload_hash: [u8; 32] = inner_parser
            .load_bits(256)
            .map_err(|err| BocParsingError(err.to_string()))?
            .try_into()
            .map_err(|_| BocParsingError("Invalid payload hash length".to_string()))?;

        let destination_address = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;

        let hash_part: TonHash = destination_address
            .clone()
            .try_into()
            .map_err(|_| "Invalid hash length")
            .unwrap();
        
        let ton_address = TonAddress::new(WORKCHAIN, hash_part);

        let destination_chain = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;



        Ok(TonCCMessage {
            message_id,
            source_chain,
            source_address,
            destination_chain,
            destination_address: ton_address.to_hex(),
            log_event: "".to_string(),
            payload_hash
        })
    }
}

#[cfg(test)]
mod tests {
    use primitive_types::H256;
    use crate::boc::cc_message::TonCCMessage;

    #[test]
    fn test_ton_log() {
        let response = TonCCMessage::from_boc_b64("te6cckEBBwEA1AAEAAECAwQAiDB4ZjM4ZDJhNjQ2ZTRiNjBlMzdiYzE2ZDU0YmI5MTYzNzM5MzcyNTk0ZGM5NmJhYjk1NGE4NWI0YTE3MGY0OWU1OC0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AkCeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwUGAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMs9py6Y=");
        assert!(
            response.is_ok(),
            "Failed to parse TonLog: {:?}",
            response.unwrap_err()
        );

        let log = response.unwrap();
        assert_eq!(
            log.message_id,
            "0xf38d2a646e4b60e37bc16d54bb9163739372594dc96bab954a85b4a170f49e58-1"
        );
        assert_eq!(log.source_chain, "avalanche-fuji");
        assert_eq!(
            log.source_address,
            "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5"
        );
        assert_eq!(log.destination_chain, "ton2");
        assert_eq!(
            log.destination_address,
            "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930"
        );

        let payload_hash = format!("{:?}", H256::from(log.payload_hash));
        assert_eq!(payload_hash, "0x9e01c423ca440c5ec2beecc9d0a152b54fc8e7a416c931b7089eaf221605af17");
    }

    #[test]
    fn test_ton_log_invalid_boc() {
        let invalid_boc = "this_is_not_a_valid_boc_string";
        let response = TonCCMessage::from_boc_b64(invalid_boc);
        assert!(response.is_err());
    }
}
