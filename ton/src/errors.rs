use thiserror::Error;

#[derive(Error, Debug)]
pub enum BocError {
    #[error("BocParsingError: {0}")]
    BocParsingError(String),
    #[error("Invalid Op Code: {0}")]
    InvalidOpCode(String),
}
