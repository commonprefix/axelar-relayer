use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use num_bigint::BigUint;
use std::sync::Arc;
use tonlib_core::cell::{Cell, CellBuilder};

const BYTES_PER_CELL: usize = 96;

fn build_cell_chain(start_index: usize, buffer: &Vec<u8>) -> Result<Cell, BocError> {
    let mut builder = CellBuilder::new();
    let end_index = std::cmp::min(start_index + BYTES_PER_CELL, buffer.len());

    for byte in buffer
        .iter()
        .skip(start_index)
        .take(end_index - start_index)
    {
        builder
            .store_uint(8, &BigUint::from(*byte))
            .map_err(|e| BocParsingError(e.to_string()))?;
    }

    // If there are more bytes, create a reference to the next cell
    if end_index < buffer.len() {
        let next_cell = build_cell_chain(end_index, buffer)?;
        builder
            .store_reference(&Arc::new(next_cell))
            .map_err(|e| BocParsingError(e.to_string()))?;
    }

    builder.build().map_err(|e| BocParsingError(e.to_string()))
}

pub fn buffer_to_cell(buffer: &Vec<u8>) -> Result<Cell, BocError> {
    build_cell_chain(0, buffer)
}
