use num_bigint::BigUint;
use tiny_keccak::{Hasher, Keccak};

pub fn payload_hash(payload: &[u8]) -> BigUint {
    let mut output = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(payload);
    hasher.finalize(&mut output);
    BigUint::from_bytes_be(&output)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use crate::hashing::BigUint;

    #[test]
    fn test_payload_hash() {
        let payload: [u8; 96] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 19, 72, 101, 108, 108, 111, 32, 102, 114, 111, 109, 32, 114, 101, 108,
            97, 121, 101, 114, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let hash = super::payload_hash(payload.as_ref());
        assert_eq!(
            hash,
            BigUint::from_str(
                "71468550630404048420691790219403539000788302635511547374558478410759778184983"
            )
            .unwrap()
        );
    }
}
