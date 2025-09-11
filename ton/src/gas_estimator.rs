/*!

This might be overly simplistic right now, we will test more to find a better way to estimate gas.

*/

use crate::config::GasEstimates;
use async_trait::async_trait;

#[derive(Clone)]
pub struct TONGasEstimator {
    config: GasEstimates,
}

impl TONGasEstimator {
    pub fn new(config: GasEstimates) -> Self {
        Self { config }
    }
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GasEstimator {
    async fn native_gas_refund_estimate(&self) -> u64;
    async fn execute_send(&self, payload: usize) -> u64;
    async fn execute_estimate(&self, payload: usize) -> u64;
    async fn approve_send(&self, num_message: usize) -> u64;
    async fn highload_wallet_send(&self, num_actions: usize) -> u64;
}

#[async_trait]
impl GasEstimator for TONGasEstimator {
    async fn native_gas_refund_estimate(&self) -> u64 {
        self.config.native_gas_refund + self.config.native_gas_refund_storage_slippage
    }

    async fn execute_estimate(&self, payload: usize) -> u64 {
        self.config.execute_base
            + self.config.execute_payload * payload as u64
            + self.config.execute_storage_slippage
    }

    async fn execute_send(&self, payload: usize) -> u64 {
        std::cmp::max(
            self.config.execute_send_min,
            self.execute_estimate(payload).await,
        )
    }

    async fn highload_wallet_send(&self, num_actions: usize) -> u64 {
        self.config.highload_wallet_send * num_actions as u64
    }

    async fn approve_send(&self, _num_messages: usize) -> u64 {
        self.config.approve_send
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GasEstimates;

    #[tokio::test]
    async fn test_native_gas_refund_estimate() {
        let config = GasEstimates {
            native_gas_refund: 100,
            native_gas_refund_storage_slippage: 20,
            execute_send_min: 1,
            execute_base: 0,
            execute_payload: 0,
            execute_storage_slippage: 1,
            approve_send: 500000000,
            highload_wallet_send: 1,
        };

        let estimator = TONGasEstimator::new(config);
        let refund = estimator.native_gas_refund_estimate().await;
        assert_eq!(refund, 120);
    }

    #[tokio::test]
    async fn test_execute_estimate() {
        let config = GasEstimates {
            native_gas_refund: 1,
            native_gas_refund_storage_slippage: 1,
            execute_send_min: 310000000,
            execute_base: 40000000,
            execute_payload: 21000,
            execute_storage_slippage: 0,
            approve_send: 500000000,
            highload_wallet_send: 1,
        };

        let estimator = TONGasEstimator::new(config);

        let execute = estimator.execute_estimate(3842usize).await;
        assert_eq!(execute, 120682000);

        let execute = estimator.execute_estimate(8000usize).await;
        assert_eq!(execute, 208000000);
    }

    #[tokio::test]
    async fn test_estimate_approve_messages() {
        let config = GasEstimates {
            native_gas_refund: 1,
            native_gas_refund_storage_slippage: 1,
            execute_send_min: 1,
            execute_base: 0,
            execute_payload: 0,
            execute_storage_slippage: 1,
            highload_wallet_send: 1,
            approve_send: 500000000,
        };

        let estimator = TONGasEstimator::new(config);
        let approve = estimator.approve_send(3usize).await;
        assert_eq!(approve, 500000000);
    }

    #[tokio::test]
    async fn test_highload_wallet_send() {
        let config = GasEstimates {
            native_gas_refund: 1,
            native_gas_refund_storage_slippage: 1,
            execute_send_min: 1,
            execute_base: 0,
            execute_payload: 0,
            execute_storage_slippage: 1,
            approve_send: 500000000,
            highload_wallet_send: 42,
        };

        let estimator = TONGasEstimator::new(config);
        let approve = estimator.highload_wallet_send(3usize).await;
        assert_eq!(approve, 126);
    }

    #[tokio::test]
    async fn test_zero_values() {
        let config = GasEstimates {
            native_gas_refund: 0,
            native_gas_refund_storage_slippage: 0,
            execute_send_min: 0,
            execute_base: 0,
            execute_payload: 0,
            execute_storage_slippage: 0,
            highload_wallet_send: 0,
            approve_send: 0,
        };

        let estimator = TONGasEstimator::new(config);
        let refund = estimator.native_gas_refund_estimate().await;
        let execute = estimator.execute_estimate(0).await;
        let approve = estimator.approve_send(5usize).await;
        let highload_wallet = estimator.highload_wallet_send(5usize).await;
        assert_eq!(refund, 0);
        assert_eq!(execute, 0);
        assert_eq!(approve, 0);
        assert_eq!(highload_wallet, 0);
    }

    #[tokio::test]
    async fn test_execute_send() {
        let config = GasEstimates {
            native_gas_refund: 0,
            native_gas_refund_storage_slippage: 0,
            execute_send_min: 100000,
            execute_base: 50000,
            execute_payload: 1000,
            execute_storage_slippage: 0,
            approve_send: 500000000,
            highload_wallet_send: 0,
        };

        let estimator = TONGasEstimator::new(config);

        // Estimated = 50000 + 1000 * 10 = 60000 < execute_send_min, should return execute_send_min
        let result = estimator.execute_send(10).await;
        assert_eq!(result, 100000);

        // Estimated = 50000 + 1000 * 200 = 250000 > execute_send_min, should return estimated
        let result = estimator.execute_send(200).await;
        assert_eq!(result, 250000);
    }
}
