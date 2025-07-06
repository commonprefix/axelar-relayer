/*!

Highload v3 wallet for TON implementation.

# Usage Example

```rust,no_run
use tonlib_core::tlb_types::block::out_action::{OutAction};
use ton::ton_wallet_high_load_v3::{TonWalletHighLoadV3};
use tonlib_core::wallet::mnemonic::KeyPair;
use tonlib_core::TonAddress;
use num_bigint::BigUint;

let actions: Vec<OutAction> = vec![/* fill with OutAction instances */];
let address = TonAddress::from_hex_str("0:0000000000000000000000000000000000000000000000000000000000000000").unwrap();
let key_pair = KeyPair {
    public_key: vec![0; 32],
    secret_key: vec![1; 64],
};

let wallet = TonWalletHighLoadV3::new(address, key_pair, 698983, 60 * 60);

let boc = wallet.outgoing_message(actions, 12345, BigUint::from(100u32));
// send using reqwest ...
```
# TODO

- [ ] Ability to repack more than 254 messages.

# Notes

Once this code has proven to be useful and all TODOs have been implemented we should try
to rewrite it so it can become a part of the tonlib core library.

# See also

- https://docs.ton.org/v3/guidelines/smart-contracts/howto/wallet#-high-load-wallet-v3

*/

use nacl::sign::signature;
use num_bigint::{BigInt, BigUint};
use std::time::{SystemTime, UNIX_EPOCH};
use tonlib_core::cell::{BagOfCells, Cell, CellBuilder, TonCellError};
use tonlib_core::message::{InternalMessage, TonMessage, TonMessageError, TransferMessage};
use tonlib_core::tlb_types::block::coins::Grams;
use tonlib_core::tlb_types::block::message::{CommonMsgInfo, ExtInMsgInfo, Message};
use tonlib_core::tlb_types::block::msg_address::{MsgAddrNone, MsgAddressExt};
use tonlib_core::tlb_types::block::out_action::{OutAction, OutList};
use tonlib_core::tlb_types::tlb::TLB;
use tonlib_core::wallet::mnemonic::KeyPair;
use tonlib_core::TonAddress;

#[derive(Debug)]
pub struct SystemTimeProvider;

pub trait TimeProvider: Send + Sync {
    fn now(&self) -> u64;
}

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs()
    }
}

#[derive(Debug)]
pub struct TonWalletHighLoadV3<T: TimeProvider = SystemTimeProvider> {
    pub(crate) address: TonAddress,
    pub(crate) key_pair: KeyPair,
    pub(crate) subwallet_id: u32,
    pub(crate) timeout: u64,
    pub(crate) time_provider: T,
}

impl TonWalletHighLoadV3<SystemTimeProvider> {
    pub fn new(address: TonAddress, key_pair: KeyPair, subwallet_id: u32, timeout: u64) -> Self {
        TonWalletHighLoadV3 {
            address,
            key_pair,
            subwallet_id,
            timeout,
            time_provider: SystemTimeProvider,
        }
    }
}

impl<T: TimeProvider> TonWalletHighLoadV3<T> {
    fn internal_transfer_body(
        &self,
        actions: Vec<OutAction>,
        query_id: u64,
    ) -> anyhow::Result<Cell, TonCellError> {
        if actions.len() > 254 {
            panic!("Max allowed action count is 254. Use pack_actions instead.");
        }

        let mut builder = CellBuilder::new();
        let out_list = OutList::new(&actions).unwrap();
        out_list.write(&mut builder).expect("TODO: panic message");
        let actions_cell = builder.build();

        let mut b = CellBuilder::new();
        b.store_u32(32, 0xae42e5a4)?
            .store_u64(64, query_id)?
            .store_reference(&actions_cell.unwrap().to_arc())?;

        b.build()
    }

    fn internal_transfer_message_cell(
        &self,
        internal_transfer_body: Cell,
        internal_message_value: BigUint,
    ) -> Result<Cell, TonMessageError> {
        let im = tonlib_core::message::CommonMsgInfo::InternalMessage(InternalMessage {
            ihr_disabled: false,
            bounce: true,
            bounced: false,
            src: TonAddress::NULL,
            dest: self.address.clone(),
            value: internal_message_value,
            ihr_fee: BigUint::from(0u32),
            fwd_fee: BigUint::from(0u32),
            created_lt: 0,
            created_at: 0,
        });

        TransferMessage::new(im, internal_transfer_body.to_arc()).build()
    }

    fn message_inner(
        &self,
        message: Cell,
        mode: u8,
        query_id: u64,
        created_at: u64,
        timeout: u64,
    ) -> Result<Cell, TonCellError> {
        let mut builder = CellBuilder::new();
        builder.store_u32(32, self.subwallet_id)?;
        builder.store_reference(&message.to_arc())?;
        builder.store_u8(8, mode)?;
        builder.store_int(23, &BigInt::from(query_id))?;
        builder.store_u64(64, created_at)?;
        builder.store_number(22, &BigInt::from(timeout))?;
        builder.build()
    }

    fn sign_external_body(&self, external_body: &Cell) -> Result<Cell, TonCellError> {
        let message_hash = external_body.cell_hash();
        let sign = signature(message_hash.as_slice(), self.key_pair.secret_key.as_slice())
            .map_err(|err| TonCellError::InternalError(err.message))?;
        let mut builder = CellBuilder::new();
        builder.store_slice(&sign)?;
        builder.store_reference(&external_body.clone().to_arc())?;
        builder.build()
    }

    fn wrap_signed_body(&self, signed_body: Cell) -> Result<Cell, TonCellError> {
        let msg_info = CommonMsgInfo::ExtIn(ExtInMsgInfo {
            src: MsgAddressExt::None(MsgAddrNone {}),
            dest: self.address.to_msg_address_int(),
            import_fee: Grams::new(BigUint::from(0u32)),
        });
        let message = Message::new(msg_info, signed_body.to_arc());
        message.to_cell()
    }

    pub fn outgoing_message(
        &self,
        actions: Vec<OutAction>,
        query_id: u64,
        internal_message_value: BigUint,
    ) -> BagOfCells {
        let internal_transfer_body = self.internal_transfer_body(actions, query_id);
        let internal_transfer = self.internal_transfer_message_cell(
            internal_transfer_body.unwrap(),
            internal_message_value,
        );

        let created_at = self.created_at();

        let message_inner = self
            .message_inner(
                internal_transfer.unwrap(),
                1,
                query_id,
                created_at,
                self.timeout,
            )
            .unwrap();
        let signed_body = self.sign_external_body(&message_inner).unwrap();

        let wrapped_signed_body = self.wrap_signed_body(signed_body).unwrap();
        BagOfCells::from_root(wrapped_signed_body)
    }

    fn created_at(&self) -> u64 {
        // LiteServers have some delay in time
        self.time_provider.now() - (self.timeout / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;
    use std::str::FromStr;
    use tonlib_core::tlb_types::block::out_action::{OutAction, OutActionSendMsg};
    use tonlib_core::wallet::mnemonic::KeyPair;
    use tonlib_core::TonAddress;

    impl<T: TimeProvider> TonWalletHighLoadV3<T> {
        pub fn new_with_time_provider(
            address: TonAddress,
            key_pair: KeyPair,
            subwallet_id: u32,
            timeout: u64,
            time_provider: T,
        ) -> Self {
            TonWalletHighLoadV3 {
                address,
                key_pair,
                subwallet_id,
                timeout,
                time_provider,
            }
        }
    }

    struct MockTimeProvider {
        fixed_time: u64,
    }

    impl TimeProvider for MockTimeProvider {
        fn now(&self) -> u64 {
            self.fixed_time
        }
    }

    fn mock_keypair() -> KeyPair {
        // This needs a valid test vector or mock setup.
        KeyPair {
            public_key: vec![0; 32],
            secret_key: vec![1; 64],
        }
    }

    fn mock_address() -> TonAddress {
        TonAddress::from_str("0:0000000000000000000000000000000000000000000000000000000000000000")
            .unwrap()
    }

    fn mock_out_action() -> OutAction {
        let mut builder = CellBuilder::new();
        builder.store_u32(32, 0x00000028u32).unwrap();

        let mut builder_inner = CellBuilder::new();

        builder_inner.store_u8(8, 42u8).unwrap();

        builder
            .store_reference(&builder_inner.build().unwrap().to_arc())
            .unwrap();
        let body = builder.build().unwrap();

        let value: BigUint = 2_000_000_000u32.into();

        let destination =
            TonAddress::from_base64_url("EQD__________________________________________0vo")
                .unwrap();

        let common =
            tonlib_core::message::CommonMsgInfo::new_default_internal(&destination, &value);

        let tm = TransferMessage::new(common, body.to_arc())
            .build()
            .unwrap()
            .to_arc();

        OutAction::SendMsg(OutActionSendMsg {
            mode: 1,
            out_msg: tm,
        })
    }

    #[test]
    fn test_internal_transfer_body_valid() {
        let actions = vec![mock_out_action()];
        let wallet = TonWalletHighLoadV3::new(mock_address(), mock_keypair(), 698983, 600);
        let result = wallet.internal_transfer_body(actions, 12345);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().to_boc_b64(true).unwrap(), "te6cckEBBgEAWwABGK5C5aQAAAAAAAAwOQECCg7DyG0BAgMAAAFoMgB//////////////////////////////////////////6O5rKAAAAAAAAAAAAAAAAAAAQQBCAAAACgFAAIqBhixuA==");
    }

    #[test]
    #[should_panic(expected = "Max allowed action count is 254")]
    fn test_internal_transfer_body_too_many_actions() {
        let actions = vec![mock_out_action(); 255];
        let wallet = TonWalletHighLoadV3::new(mock_address(), mock_keypair(), 698983, 600);
        let _ = wallet.internal_transfer_body(actions, 1);
    }

    #[test]
    fn test_internal_transfer_message_cell_builds() {
        let actions = vec![mock_out_action()];
        let wallet = TonWalletHighLoadV3::new(mock_address(), mock_keypair(), 698983, 600);
        let body = wallet.internal_transfer_body(actions, 42).unwrap();
        let internal = wallet.internal_transfer_message_cell(body, BigUint::from(100u32));
        assert!(internal.is_ok());
        assert_eq!(internal.unwrap().to_boc_b64(true).unwrap(), "te6cckEBBwEAbgABHyAWQAAAAAAAAAAAAAAAAAMBARiuQuWkAAAAAAAAACoCAgoOw8htAQMEAAABaDIAf/////////////////////////////////////////+juaygAAAAAAAAAAAAAAAAAAEFAQgAAAAoBgACKifI9bE=");
    }

    #[test]
    fn test_message_inner_structure() {
        let wallet = TonWalletHighLoadV3::new(mock_address(), mock_keypair(), 123456, 555);
        let message = CellBuilder::new().build().unwrap();
        let result = wallet.message_inner(message, 0b10101010, 777, 1000, 500);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().to_boc_b64(true).unwrap(),
            "te6cckEBAgEAGAABJQAB4kCqAAYSAAAAAAAAB9AAD6QBAACWEx7j"
        );
    }

    #[test]
    fn test_sign_external_body() {
        let wallet = TonWalletHighLoadV3::new(mock_address(), mock_keypair(), 123, 100);
        let cell = CellBuilder::new().store_u8(8, 42).unwrap().build().unwrap();
        let result = wallet.sign_external_body(&cell);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().to_boc_b64(true).unwrap(), "te6cckEBAgEARgABgLB92OjwqNwYG9CjDeue+0oYguCYmhkWizg+KFQCmWlDYbif4prV1S6MuFbfb1kDZ9DgjD3oePTm41w+83i7fggBAAIqbix/vA==");
    }

    #[test]
    fn test_wrap_signed_body_builds() {
        let wallet = TonWalletHighLoadV3::new(mock_address(), mock_keypair(), 123, 100);
        let cell = CellBuilder::new().store_u8(8, 55).unwrap().build().unwrap();
        let result = wallet.wrap_signed_body(cell);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().to_boc_b64(true).unwrap(),
            "te6cckEBAQEAJgAAR4gAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABvAFKRlA="
        );
    }

    #[test]
    fn test_outgoing_message_success() {
        let mock_time = MockTimeProvider { fixed_time: 100000 };
        let wallet = TonWalletHighLoadV3::new_with_time_provider(
            mock_address(),
            mock_keypair(),
            321,
            500,
            mock_time,
        );
        let boc = wallet.outgoing_message(vec![mock_out_action()], 42, BigUint::from(999u32));
        assert_eq!(boc.root(0).unwrap().to_boc_b64(true).unwrap(), "te6cckEBCQEA6wABxYgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACXCFa6bM55U2zArv5G9cdeG77TKolB1hGNn/4Am2aaNiv4r7ZGkgBtIOl4vL8MhFFQdTIkMn/hdwNwGLhJ69wTAEBJQAAAUEBAABUAAAAAAADDTAAD6QCASEgID5wAAAAAAAAAAAAAAAAAwMBGK5C5aQAAAAAAAAAKgQCCg7DyG0BBQYAAAFoMgB//////////////////////////////////////////6O5rKAAAAAAAAAAAAAAAAAAAQcBCAAAACgIAAIq0VehOw==");
    }
}
