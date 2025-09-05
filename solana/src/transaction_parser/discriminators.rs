pub const CPI_EVENT_DISC: [u8; 8] = [228, 69, 165, 46, 81, 203, 154, 29]; // 0xe445a52e51cb9a1d (EVENT_IX_TAG in LE)

pub const NATIVE_GAS_PAID_EVENT_DISC: [u8; 8] = [232, 125, 221, 19, 212, 213, 137, 199]; // sha256(b"event:NativeGasPaidForContractCallEvent")
pub const NATIVE_GAS_ADDED_EVENT_DISC: [u8; 8] = [163, 149, 4, 134, 245, 100, 202, 54]; // sha256(b"event:NativeGasAddedForContractCallEvent")
pub const NATIVE_GAS_REFUNDED_EVENT_DISC: [u8; 8] = [43, 222, 83, 117, 24, 237, 201, 202]; // sha256(b"event:NativeRefundedEvent")
