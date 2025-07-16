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
    DataError(String)
}


#[derive(Error, Debug)]
pub enum GasError {
    #[error("GasCalculationError: {0}")]
    GasCalculationError(String)
}
