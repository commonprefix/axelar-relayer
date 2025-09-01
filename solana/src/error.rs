use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransactionParsingError {
    // #[error("BocParsingError: {0}")]
    // BocParsing(String),
    #[error("MessageParsingError: {0}")]
    Message(String),
    #[error("GasError: {0}")]
    Gas(String),
    #[error("GeneralError: {0}")]
    Generic(String),
}

#[derive(Error, Debug)]
pub enum GasError {
    #[error("ConversionError: {0}")]
    ConversionError(String),
    #[error("GasCalculationError: {0}")]
    GasCalculationError(String),
}
