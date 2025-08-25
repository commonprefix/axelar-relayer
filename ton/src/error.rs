use thiserror::Error;

#[derive(Error, Debug)]
pub enum BocError {
    #[error("BocEncodingError: {0}")]
    BocEncodingError(String),
    #[error("BocParsingError: {0}")]
    BocParsingError(String),
    #[error("Invalid Op Code: {0}")]
    InvalidOpCode(String),
}

#[derive(Error, Debug)]
pub enum TONRpcError {
    #[error("DataError: {0}")]
    DataError(String),
}

#[derive(Error, Debug)]
pub enum GasError {
    #[error("ConversionError: {0}")]
    ConversionError(String),
    #[error("GasCalculationError: {0}")]
    GasCalculationError(String),
}

#[derive(Error, Debug)]
pub enum TransactionParsingError {
    #[error("BocParsingError: {0}")]
    BocParsing(String),
    #[error("MessageParsingError: {0}")]
    Message(String),
    #[error("GasError: {0}")]
    Gas(String),
    #[error("ITSWithoutPair: {0}")]
    ITSWithoutPair(String),
    #[error("GeneralError: {0}")]
    Generic(String),
}
