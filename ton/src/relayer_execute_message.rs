/*!
# TODO:

- Check for errors
*/

use std::sync::Arc;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};
use tonlib_core::cell::{Cell, CellBuilder};
use tonlib_core::TonAddress;

const BYTES_PER_CELL: usize = 96;

fn build_cell_chain(start_index: usize, buffer: &Vec<u8>) -> Cell {
    let mut builder = CellBuilder::new();
    let end_index = std::cmp::min(start_index + BYTES_PER_CELL, buffer.len());

    // Store bytes in the current cell
    for i in start_index..end_index {
        builder.store_uint(8, &BigUint::from(buffer[i])).unwrap();
    }

    // If there are more bytes, create a reference to the next cell
    if end_index < buffer.len() {
        let next_cell = build_cell_chain(end_index, buffer);
        builder.store_reference(&Arc::new(next_cell)).unwrap();
    }
    
    builder.build().unwrap()
}

fn buffer_to_cell(buffer: &Vec<u8>) -> Cell {
    build_cell_chain(0, buffer)
}


#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RelayerExecuteMessage {
    message_id: String,
    source_chain: String,
    source_address: String,
    destination_chain: String,
    destination_address: String,
    payload: String,
    relayer_address: TonAddress,
}

impl RelayerExecuteMessage {
    pub fn new(
        message_id: String,
        source_chain: String,
        source_address: String,
        destination_chain: String,
        destination_address: String,
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
            relayer_address
        }
    }

    fn payload_hash(payload: &Vec<u8>) -> BigUint {
        let mut output = [0u8; 32];
        let mut hasher = Keccak::v256();
        hasher.update(payload);
        hasher.finalize(&mut output);
        println!("Calculated payload hash as {:?}", hex::encode(output));
        BigUint::from_bytes_be(&output)
    }
    
    fn to_cell(&self) -> Cell {
        let message_id = buffer_to_cell(&self.message_id.as_bytes().to_vec());
        let source_chain = buffer_to_cell(&self.source_chain.as_bytes().to_vec());
        let source_contract_address = buffer_to_cell(&self.source_address.as_bytes().to_vec());
        let destination_chain = buffer_to_cell(&self.destination_chain.as_bytes().to_vec());
        let destination_address = buffer_to_cell(&self.destination_address.as_bytes().to_vec());


        let payload_hash = Self::payload_hash(&self.payload.as_bytes().to_vec());
        let payload = buffer_to_cell(&self.payload.as_bytes().to_vec());

        let mut inner = CellBuilder::new();
        inner.store_reference(&Arc::new(payload)).unwrap();
        inner.store_reference(&Arc::new(destination_address)).unwrap();
        inner.store_reference(&Arc::new(destination_chain)).unwrap();
        inner.store_uint(256, &payload_hash).unwrap();
        let inner = inner.build().unwrap();

        let mut message = CellBuilder::new();
        message.store_reference(&Arc::new(message_id)).unwrap();
        message.store_reference(&Arc::new(source_chain)).unwrap();
        message.store_reference(&Arc::new(source_contract_address)).unwrap();
        message.store_reference(&Arc::new(inner)).unwrap();
        let message = message.build().unwrap();

        let mut outer = CellBuilder::new();
        outer.store_uint(32, &BigUint::from(0x00000008u32)).unwrap();
        outer.store_reference(&Arc::new(message)).unwrap();
        outer.store_address(&self.relayer_address).unwrap();
        let outer = outer.build().unwrap();

        outer
    }
}
