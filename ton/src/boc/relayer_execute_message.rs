/*!

# Usage Example

```rust
use tonlib_core::TonAddress;
use ton::boc::relayer_execute_message::RelayerExecuteMessage;

let relayer_address: TonAddress = "0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b".parse().unwrap();
let address: TonAddress = "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930".parse().unwrap();

let message = RelayerExecuteMessage::new(
    "0x8ccca7622377b11ac745117784242dbae8694b4a84495dacffde08af738db9a0-1".to_string(),
    "avalanche-fuji".to_string(),
    "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5".to_string(),
    "ton2".to_string(),
    address,
    "0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001348656c6c6f2066726f6d2072656c617965722100000000000000000000000000".to_string(),
    relayer_address,
);

let ton_cell = message.to_cell().unwrap();
```

# TODO:
- Don't hardcode execute message gas - waiting for the chain contract fix

*/

use crate::boc::buffer_to_cell;
use crate::error::BocError;
use crate::error::BocError::{BocEncodingError, BocParsingError};
use crate::ton_constants::OP_RELAYER_EXECUTE;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tiny_keccak::{Hasher, Keccak};
use tonlib_core::cell::{Cell, CellBuilder};
use tonlib_core::TonAddress;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RelayerExecuteMessage {
    pub(crate) message_id: String,
    pub(crate) source_chain: String,
    pub(crate) source_address: String,
    pub(crate) destination_chain: String,
    pub(crate) destination_address: TonAddress,
    pub(crate) payload: String,
    relayer_address: TonAddress,
}

impl RelayerExecuteMessage {
    pub fn new(
        message_id: String,
        source_chain: String,
        source_address: String,
        destination_chain: String,
        destination_address: TonAddress,
        payload: String,
        relayer_address: TonAddress,
    ) -> Self {
        Self {
            message_id,
            source_chain,
            source_address,
            destination_chain,
            destination_address,
            payload,
            relayer_address,
        }
    }

    fn payload_hash(payload: &[u8]) -> BigUint {
        let mut output = [0u8; 32];
        let mut hasher = Keccak::v256();
        hasher.update(payload);
        hasher.finalize(&mut output);
        BigUint::from_bytes_be(&output)
    }

    pub fn to_cell(&self) -> Result<Cell, BocError> {
        let message_id = buffer_to_cell::buffer_to_cell(&self.message_id.as_bytes().to_vec())?;
        let source_chain = buffer_to_cell::buffer_to_cell(&self.source_chain.as_bytes().to_vec())?;
        let source_address =
            buffer_to_cell::buffer_to_cell(&self.source_address.as_bytes().to_vec())?;
        let destination_chain =
            buffer_to_cell::buffer_to_cell(&self.destination_chain.as_bytes().to_vec())?;
        let destination_address =
            buffer_to_cell::buffer_to_cell(&self.destination_address.hash_part.to_vec())?;
        let payload_bytes = hex::decode(&self.payload)
            .map_err(|e| BocParsingError(format!("Invalid hex payload: {}", e)))?;

        let payload_hash = Self::payload_hash(&payload_bytes);
        let payload = buffer_to_cell::buffer_to_cell(&payload_bytes)?;

        let mut inner = CellBuilder::new();
        inner
            .store_reference(&Arc::new(payload))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        inner
            .store_reference(&Arc::new(destination_address))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        inner
            .store_reference(&Arc::new(destination_chain))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        inner
            .store_uint(256, &payload_hash)
            .map_err(|e| BocEncodingError(e.to_string()))?;

        let inner = inner.build().map_err(|e| BocParsingError(e.to_string()))?;

        let mut message = CellBuilder::new();
        message
            .store_reference(&Arc::new(message_id))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        message
            .store_reference(&Arc::new(source_chain))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        message
            .store_reference(&Arc::new(source_address))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        message
            .store_reference(&Arc::new(inner))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        let message = message
            .build()
            .map_err(|e| BocEncodingError(e.to_string()))?;

        let mut outer = CellBuilder::new();
        outer
            .store_uint(32, &BigUint::from(OP_RELAYER_EXECUTE))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        outer
            .store_reference(&Arc::new(message))
            .map_err(|e| BocEncodingError(e.to_string()))?;
        outer
            .store_address(&self.relayer_address)
            .map_err(|e| BocEncodingError(e.to_string()))?;
        outer.build().map_err(|e| BocEncodingError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigUint;
    use std::str::FromStr;
    use tonlib_core::tlb_types::tlb::TLB;
    use tonlib_core::TonAddress;

    #[test]
    fn test_payload_hash() {
        let payload: [u8; 96] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 19, 72, 101, 108, 108, 111, 32, 102, 114, 111, 109, 32, 114, 101, 108,
            97, 121, 101, 114, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let hash = super::RelayerExecuteMessage::payload_hash(payload.as_ref());
        assert_eq!(
            hash,
            BigUint::from_str(
                "71468550630404048420691790219403539000788302635511547374558478410759778184983"
            )
            .unwrap()
        );
    }

    #[test]
    fn test_to_cell() {
        let relayer_address: TonAddress = "0QCQPVhDBzLBwIlt8MtDhPwIrANfNH2ZQnX0cSvhCD4Dld4b"
            .parse()
            .unwrap();

        let address: TonAddress =
            "0:b87a4a0f644b7a186ee71a1454634f70c22a62aca1a6ba676b5175c21d7fd930"
                .parse()
                .unwrap();

        let message = super::RelayerExecuteMessage::new(
            "0x8ccca7622377b11ac745117784242dbae8694b4a84495dacffde08af738db9a0-1".to_string(),
            "avalanche-fuji".to_string(),
            "0xd7067Ae3C359e837890b28B7BD0d2084CfDf49b5".to_string(),
            "ton2".to_string(),
            address,
            "0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001348656c6c6f2066726f6d2072656c617965722100000000000000000000000000".to_string(),
            relayer_address
        );

        let res = message.to_cell().unwrap();
        assert_eq!(res.to_boc_b64(true).unwrap(), "te6cckECCQEAAWAAAUsAAAAIgBIHqwhg5lg4ES2+GWhwn4EVgGvmj7MoTr6OJXwhB8BysAEEAAIDBAUAiDB4OGNjY2E3NjIyMzc3YjExYWM3NDUxMTc3ODQyNDJkYmFlODY5NGI0YTg0NDk1ZGFjZmZkZTA4YWY3MzhkYjlhMC0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1A0CeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwYHCADAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gcmVsYXllciEAAAAAAAAAAAAAAAAAAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMgKZxA0=");
    }
}
