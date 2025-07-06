/*!

Query Id manager for high load wallet manager.

# Usage Example

```rust
use ton::high_load_query_id::HighLoadQueryId;

#[tokio::main]
async fn main() {
    let query_id = HighLoadQueryId::from_shift_and_bitnumber(0, 0).await.unwrap();
    let next_query_id = query_id.next().await.unwrap();
}
```

# See also

- https://docs.ton.org/v3/guidelines/smart-contracts/howto/wallet#shifts-and-bits-numbers-as-query-id

*/

#[derive(Debug, PartialEq)]
pub enum HighLoadQueryIdError {
    InvalidShift(u32),
    InvalidBitnumber(u32),
    EmergencyOverload,
    ShiftOverflow,
}

#[derive(Debug, Clone)]
pub struct HighLoadQueryId {
    pub shift: u32,     // [0 .. 8191]
    pub bitnumber: u32, // [0 .. 1022]
}

impl HighLoadQueryId {
    const MAX_SHIFT: u32 = 8191;
    const MAX_BITNUMBER: u32 = 1022;
    const BITNUMBER_SIZE: u8 = 10;

    pub async fn from_shift_and_bitnumber(
        shift: u32,
        bitnumber: u32,
    ) -> Result<Self, HighLoadQueryIdError> {
        if shift > Self::MAX_SHIFT {
            return Err(HighLoadQueryIdError::InvalidShift(Self::MAX_SHIFT));
        }
        if bitnumber > Self::MAX_BITNUMBER {
            return Err(HighLoadQueryIdError::InvalidBitnumber(Self::MAX_BITNUMBER));
        }

        Ok(Self { shift, bitnumber })
    }

    pub async fn next(&self) -> Result<Self, HighLoadQueryIdError> {
        let mut new_bitnumber = self.bitnumber + 1;
        let mut new_shift = self.shift;

        if new_shift == Self::MAX_SHIFT && new_bitnumber == (Self::MAX_BITNUMBER - 1) {
            return Err(HighLoadQueryIdError::EmergencyOverload);
        }

        if new_bitnumber > Self::MAX_BITNUMBER {
            new_bitnumber = 0;
            new_shift += 1;

            if new_shift > Self::MAX_SHIFT {
                return Err(HighLoadQueryIdError::ShiftOverflow);
            }
        }

        HighLoadQueryId::from_shift_and_bitnumber(new_shift, new_bitnumber).await
    }

    pub async fn has_next(&self) -> bool {
        !(self.bitnumber >= Self::MAX_BITNUMBER - 1 && self.shift == Self::MAX_SHIFT)
    }

    pub async fn query_id(&self) -> u64 {
        ((self.shift as u64) << Self::BITNUMBER_SIZE) + self.bitnumber as u64
    }
}

#[cfg(test)]
mod tests {
    use super::{HighLoadQueryId, HighLoadQueryIdError};

    #[tokio::test]
    async fn test_new_valid() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(100, 500)
            .await
            .unwrap();
        assert_eq!(id.shift, 100);
        assert_eq!(id.bitnumber, 500);
    }

    #[tokio::test]
    async fn test_new_invalid_shift() {
        let result =
            HighLoadQueryId::from_shift_and_bitnumber(HighLoadQueryId::MAX_SHIFT + 1, 0).await;
        assert_eq!(
            result.unwrap_err(),
            HighLoadQueryIdError::InvalidShift(HighLoadQueryId::MAX_SHIFT)
        );
    }

    #[tokio::test]
    async fn test_new_invalid_bitnumber() {
        let result =
            HighLoadQueryId::from_shift_and_bitnumber(0, HighLoadQueryId::MAX_BITNUMBER + 1).await;
        assert_eq!(
            result.unwrap_err(),
            HighLoadQueryIdError::InvalidBitnumber(HighLoadQueryId::MAX_BITNUMBER)
        );
    }

    #[tokio::test]
    async fn test_next_normal() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(5, 1000)
            .await
            .unwrap();
        let next = id.next().await.unwrap();
        assert_eq!(next.shift, 5);
        assert_eq!(next.bitnumber, 1001);
    }

    #[tokio::test]
    async fn test_next_rollover_bitnumber() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(10, HighLoadQueryId::MAX_BITNUMBER)
            .await
            .unwrap();
        let next = id.next().await.unwrap();
        assert_eq!(next.shift, 11);
        assert_eq!(next.bitnumber, 0);
    }

    #[tokio::test]
    async fn test_next_emergency_overload() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(
            HighLoadQueryId::MAX_SHIFT,
            HighLoadQueryId::MAX_BITNUMBER - 2,
        )
        .await
        .unwrap();
        let result = id.next().await;
        assert_eq!(result.unwrap_err(), HighLoadQueryIdError::EmergencyOverload);
    }

    #[tokio::test]
    async fn test_next_shift_overflow() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(
            HighLoadQueryId::MAX_SHIFT,
            HighLoadQueryId::MAX_BITNUMBER,
        )
        .await
        .unwrap();
        let result = id.next().await;
        assert_eq!(result.unwrap_err(), HighLoadQueryIdError::ShiftOverflow);
    }

    #[tokio::test]
    async fn test_has_next_true() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(
            HighLoadQueryId::MAX_SHIFT - 1,
            HighLoadQueryId::MAX_BITNUMBER,
        )
        .await
        .unwrap();
        assert!(id.has_next().await);
    }

    #[tokio::test]
    async fn test_has_next_false() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(
            HighLoadQueryId::MAX_SHIFT,
            HighLoadQueryId::MAX_BITNUMBER - 1,
        )
        .await
        .unwrap();
        assert!(!id.has_next().await);
    }

    #[tokio::test]
    async fn test_query_id() {
        let id = HighLoadQueryId::from_shift_and_bitnumber(2, 3)
            .await
            .unwrap();
        let expected = ((2u64) << HighLoadQueryId::BITNUMBER_SIZE) + 3;
        assert_eq!(id.query_id().await, expected);
    }
}
