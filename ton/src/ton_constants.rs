pub const OP_APPROVE_MESSAGES: u32 = 0x00000028;
pub const OP_MESSAGE_APPROVED: u32 = 0x0000000D;
pub const OP_NULLIFIED_SUCCESSFULLY: u32 = 0x00000005;
pub const OP_MESSAGE_APPROVE: u32 = 0x00000007;
pub const OP_NULLIFY_IF_APPROVED: u32 = 0x00000004;
pub const OP_RELAYER_EXECUTE: u32 = 0x00000008;
pub const OP_GATEWAY_EXECUTE: u32 = 0x0000000C;
pub const OP_CALL_CONTRACT: u32 = 0x00000009;
pub const OP_PAY_GAS: u32 = 0x00000045;
pub const OP_PAY_NATIVE_GAS_FOR_CONTRACT_CALL: u32 = 0x0000001F;
pub const OP_ADD_NATIVE_GAS: u32 = 0x00000036;
pub const OP_NATIVE_REFUND: u32 = 0x00000046;
pub const OP_REFUND: u32 = 0x00000018;
pub const OP_USER_BALANCE_SUBTRACTED: u32 = 0x00000024;

pub const EXIT_CODE_INSUFFICIENT_GAS: u32 = 106;

pub const SEND_MODE_ORDINARY_MESSAGE: u8 = 0;
pub const SEND_MODE_CARRY_VALUE: u8 = 64;
pub const SEND_MODE_CARRY_BALANCE: u8 = 128;
pub const SEND_MODE_PAY_TRANSFER_FEES_SEPARATELY: u8 = 1;
pub const SEND_MODE_IGNORE_ERRORS: u8 = 2;
pub const SEND_MODE_BOUNCE_ON_ACTION_FAILURE: u8 = 16;
pub const SEND_MODE_DESTROY_ACCOUNT_ON_ZERO_BALANCE: u8 = 32;

pub const REFUND_DUST: u32 = 10_000_000;

pub const WORKCHAIN: i32 = 0;
