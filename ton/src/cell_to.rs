/*!

Dominik's code. We should move it to a common repo.

*/

use crate::approve_message::ApproveMessagesError;
use crate::approve_message::ApproveMessagesError::BocParsingError;
use std::sync::Arc;
use tonlib_core::cell::Cell;

const BYTES_PER_CELL: usize = 96;

pub trait CellTo {
    fn cell_to_string(self) -> anyhow::Result<String, ApproveMessagesError>;

    fn cell_to_buffer(self) -> anyhow::Result<Vec<u8>, ApproveMessagesError>;
}

impl CellTo for Arc<Cell> {
    fn cell_to_string(self) -> Result<String, ApproveMessagesError> {
        let bytes = self.cell_to_buffer()?;
        String::from_utf8(bytes).map_err(|e| BocParsingError(e.to_string()))
    }

    fn cell_to_buffer(self) -> Result<Vec<u8>, ApproveMessagesError> {
        let mut current_cell = Some(self);
        let mut u8_vec = Vec::new();

        while let Some(cell) = current_cell {
            let mut parser = cell.parser();
            for _ in 0..BYTES_PER_CELL {
                match parser.load_uint(8) {
                    Ok(val) => u8_vec.push(val.to_bytes_be()[0]),
                    Err(_) => break, // no more bytes
                }
            }
            current_cell = parser.next_reference().ok();
        }

        Ok(u8_vec)
    }
}
