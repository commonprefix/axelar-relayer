/*!
Utility to create an out_action for High Load Wallet v3.

# Usage Example
```rust,no_run
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use tonlib_core::TonAddress;
use ton::out_action::out_action;
use num_bigint::BigUint;

let approve_message = hex::encode(BASE64_STANDARD.decode("te6cck...").unwrap());
let value = 1_000_000u32;
let destination = TonAddress::from_base64_url("...").unwrap();
let res = out_action(&approve_message, BigUint::from(value), destination).unwrap();
```

*/

use crate::ton_constants::SEND_MODE_IGNORE_ERRORS;
use num_bigint::BigUint;
use tonlib_core::cell::Cell;
use tonlib_core::message::{InternalMessage, TonMessage, TonMessageError, TransferMessage};
use tonlib_core::tlb_types::block::out_action::{OutAction, OutActionSendMsg};
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::TonAddress;

pub fn out_action(
    boc_hex: &str,
    value: BigUint,
    destination: TonAddress,
) -> Result<OutAction, TonMessageError> {
    let body = Cell::from_boc_hex(boc_hex)?.to_arc();
    let common = tonlib_core::message::CommonMsgInfo::InternalMessage(InternalMessage {
        ihr_disabled: false,
        bounce: true,
        bounced: false,
        src: TonAddress::NULL,
        dest: destination,
        value,
        ihr_fee: BigUint::from(0u8),
        fwd_fee: BigUint::from(0u8),
        created_lt: 0,
        created_at: 0,
    });

    let tm = TransferMessage::new(common, body).build()?.to_arc();

    Ok(OutAction::SendMsg(OutActionSendMsg {
        mode: SEND_MODE_IGNORE_ERRORS,
        out_msg: tm,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::prelude::BASE64_STANDARD;
    use base64::Engine;

    #[test]
    fn test_out_action() {
        let approve_message = hex::encode(BASE64_STANDARD.decode("te6cckECDAEAAYsAAggAAAAoAQIBYYAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADf5gkADAQHABADi0LAAUYmshNOh1nWEdwB3eJHd51H6EH1kg3v2M30y32eQAAAAAAAAAAAAAAAAAAAAAQ+j+g0KWjWTaPqB9qQHuWZQn7IPz7x3xzwbprT1a85sjh0UlPlFU84LDdRcD4GZ6n6GJlEKKTlRW5QtlzKGrAsBAtAFBECeAcQjykQMXsK+7MnQoVK1T8jnpBbJMbcInq8iFgWvFwYHCAkAiDB4MTdmZDdkYTNkODE5Y2ZiYzQ2ZmYyOGYzZDgwOTgwNzcwZWMxYjgwZmQ3ZDFiMjI5Y2VjMzI1MTkzOWI5YjIzZi0xABxhdmFsYW5jaGUtZnVqaQBUMHhkNzA2N0FlM0MzNTllODM3ODkwYjI4QjdCRDBkMjA4NENmRGY0OWI1AgAKCwBAuHpKD2RLehhu5xoUVGNPcMIqYqyhprpna1F1wh1/2TAACHRvbjJLddsV").unwrap());
        let value = 1_000_000u32;
        let destination =
            TonAddress::from_base64_url("EQD__________________________________________0vo")
                .unwrap();

        let res = out_action(&approve_message, BigUint::from(value), destination).unwrap();
        assert_eq!(res.to_boc_b64(true).unwrap(), "te6cckECDgEAAckAAQoOw8htAgEBZiIAf/////////////////////////////////////////+YehIAAAAAAAAAAAAAAAAAAQICCAAAACgDBAFhgAAAAAAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAN/mCQAUBAcAGAOLQsABRiayE06HWdYR3AHd4kd3nUfoQfWSDe/YzfTLfZ5AAAAAAAAAAAAAAAAAAAAABD6P6DQpaNZNo+oH2pAe5ZlCfsg/PvHfHPBumtPVrzmyOHRSU+UVTzgsN1FwPgZnqfoYmUQopOVFblC2XMoasCwEC0AcEQJ4BxCPKRAxewr7sydChUrVPyOekFskxtwieryIWBa8XCAkKCwCIMHgxN2ZkN2RhM2Q4MTljZmJjNDZmZjI4ZjNkODA5ODA3NzBlYzFiODBmZDdkMWIyMjljZWMzMjUxOTM5YjliMjNmLTEAHGF2YWxhbmNoZS1mdWppAFQweGQ3MDY3QWUzQzM1OWU4Mzc4OTBiMjhCN0JEMGQyMDg0Q2ZEZjQ5YjUCAAwNAEC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX/ZMAAIdG9uMpDtUgc=");
    }
}
