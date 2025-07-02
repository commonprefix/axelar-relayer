use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;
use crate::approve_message::{ApproveMessages, ApproveMessagesError};
use crate::approve_message::ApproveMessagesError::BocParsingError;
use crate::cell_to::CellTo;

pub struct TonLog {
    message_id: String,
    log_event: String
}

impl TonLog {
    pub fn from_boc_b64(boc_b64: &str) -> Result<Self, ApproveMessagesError> {
        let cell = Cell::from_boc_b64(boc_b64).unwrap();
        let mut parser = cell.parser();

        let message_id = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;

        println!("message_id: {}", message_id);
        let source_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        println!("source_chain: {}", source_chain);
        let source_address = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        println!("source_address: {}", source_address);
        let inner_cell = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut inner_parser: CellParser = inner_cell.parser();
        let destination_address = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_buffer()?;
        println!("destination_address: {:?}", destination_address);
        let destination_chain = inner_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        println!("destination_chain: {}", destination_chain);

        let a = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()?;
        println!("a: {}", a);

        Ok(TonLog {
            message_id,
            log_event: "".to_string(),
        })

    }
}


#[cfg(test)]
mod tests {
    use crate::extract_log::TonLog;

    #[test]
    fn test_ton_log() {
        TonLog::from_boc_b64("te6cckEBBwEA1AAEAAECAwQAiDB4ZjM4ZDJhNjQ2ZTRiNjBlMzdiYzE2ZDU0YmI5MTYzNzM5MzcyNTk0ZGM5NmJhYjk1NGE4NWI0YTE3MGY0OWU1OC0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AkCeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwUGAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMs9py6Y=");
    }
}