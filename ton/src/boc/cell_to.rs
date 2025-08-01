/*!
Utility for working with TON Cell structures. Dominik's code.

# Example Usage:

```rust
use tonlib_core::cell::{CellBuilder};
use std::sync::Arc;
use ton::boc::cell_to::CellTo;

let mut builder = CellBuilder::new();
for &byte in b"Hello" {
    builder.store_byte(byte).unwrap();
}
let cell = Arc::new(builder.build().unwrap());

let string = cell.clone().cell_to_string().unwrap();
assert_eq!(string, "Hello");

let buffer = cell.cell_to_buffer().unwrap();
assert_eq!(buffer, b"Hello");
```
*/

use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use std::sync::Arc;
use tonlib_core::cell::Cell;

const BYTES_PER_CELL: usize = 96;

pub trait CellTo {
    fn cell_to_string(self) -> anyhow::Result<String, BocError>;

    fn cell_to_buffer(self) -> anyhow::Result<Vec<u8>, BocError>;
}

impl CellTo for Arc<Cell> {
    fn cell_to_string(self) -> Result<String, BocError> {
        let bytes = self.cell_to_buffer()?;
        String::from_utf8(bytes).map_err(|e| BocParsingError(e.to_string()))
    }

    fn cell_to_buffer(self) -> Result<Vec<u8>, BocError> {
        let mut current_cell = Some(self);
        let mut u8_vec = Vec::new();

        while let Some(cell) = current_cell {
            let mut parser = cell.parser();

            for _ in 0..BYTES_PER_CELL {
                match parser.load_uint(8) {
                    Ok(val) => {
                        let bytes = val.to_bytes_be();
                        if let Some(byte) = bytes.last() {
                            u8_vec.push(*byte);
                        }
                    }
                    Err(_) => break, // no more bytes
                }
            }

            current_cell = parser.next_reference().ok();
        }

        Ok(u8_vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonlib_core::cell::{Cell, CellBuilder};

    fn create_test_cell(data: &[u8]) -> Arc<Cell> {
        let mut builder = CellBuilder::new();
        for &byte in data {
            builder.store_byte(byte).expect("Failed to store byte");
        }
        Arc::new(builder.build().expect("Failed to build cell"))
    }

    #[test]
    fn test_cell_to_buffer() {
        let data = b"Test";
        let cell = create_test_cell(data);
        let result = cell.cell_to_buffer();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn test_cell_to_string_success() {
        let data = b"HelloWorld";
        let cell = create_test_cell(data);
        let result = cell.cell_to_string();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "HelloWorld");
    }

    #[test]
    fn test_cell_to_string_invalid_utf8() {
        let data = vec![0xff, 0xfe, 0xfd];
        let cell = create_test_cell(&data);
        let result = cell.cell_to_string();

        assert!(result.is_err());
    }
}
