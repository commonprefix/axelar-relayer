use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueuedTxMonitorError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("XRPL client error: {0}")]
    XRPLClientError(String),
    #[error("Transaction status check failed: {0}")]
    TransactionStatusCheckFailed(String),
    #[error("Generic error: {0}")]
    GenericError(String),
}
