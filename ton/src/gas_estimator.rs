/*!

This might be overly simplistic right now, we will test more to find a better way to estimate gas.

*/

use crate::config::GasEstimates;
use std::future::Future;

pub struct TONGasEstimator {
    config: GasEstimates,
}

impl TONGasEstimator {
    pub fn new(config: GasEstimates) -> Self {
        Self { config }
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait GasEstimator {
    fn estimate_native_gas_refund(&self) -> impl Future<Output = u64>;
    fn estimate_execute(&self) -> impl Future<Output = u64>;
    fn estimate_approve_messages(&self, num_message: usize) -> impl Future<Output = u64>;

    fn estimate_highload_wallet(&self, num_actions: usize) -> impl Future<Output = u64>;
}

impl GasEstimator for TONGasEstimator {
    async fn estimate_native_gas_refund(&self) -> u64 {
        self.config.native_gas_refund + self.config.native_gas_refund_storage_slippage
    }

    async fn estimate_execute(&self) -> u64 {
        self.config.execute + self.config.execute_storage_slippage
    }

    async fn estimate_highload_wallet(&self, num_actions: usize) -> u64 {
        self.config.highload_wallet_per_action * num_actions as u64
    }

    async fn estimate_approve_messages(&self, num_messages: usize) -> u64 {
        self.config.approve_fixed
            + self.config.approve_fixed_storage_slippage
            + self.config.approve_per_message * num_messages as u64
            + self.config.approve_per_message_storage_slippage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GasEstimates;

    #[tokio::test]
    async fn test_estimate_native_gas_refund() {
        let config = GasEstimates {
            native_gas_refund: 100,
            native_gas_refund_storage_slippage: 20,
            execute: 1,
            execute_storage_slippage: 1,
            approve_fixed: 1,
            approve_fixed_storage_slippage: 1,
            approve_per_message: 1,
            approve_per_message_storage_slippage: 1,
            highload_wallet_per_action: 1
        };

        let estimator = TONGasEstimator::new(config);
        let refund = estimator.estimate_native_gas_refund().await;
        assert_eq!(refund, 120);
    }

    #[tokio::test]
    async fn test_estimate_execute() {
        let config = GasEstimates {
            native_gas_refund: 1,
            native_gas_refund_storage_slippage: 1,
            execute: 200,
            execute_storage_slippage: 50,
            approve_fixed: 1,
            approve_fixed_storage_slippage: 1,
            approve_per_message: 1,
            approve_per_message_storage_slippage: 1,
            highload_wallet_per_action: 1
        };

        let estimator = TONGasEstimator::new(config);
        let execute = estimator.estimate_execute().await;
        assert_eq!(execute, 250);
    }

    #[tokio::test]
    async fn test_estimate_approve_messages() {
        let config = GasEstimates {
            native_gas_refund: 1,
            native_gas_refund_storage_slippage: 1,
            execute: 1,
            execute_storage_slippage: 1,
            approve_fixed: 200,
            approve_fixed_storage_slippage: 300,
            approve_per_message: 33,
            approve_per_message_storage_slippage: 22,
            highload_wallet_per_action: 1
        };

        let estimator = TONGasEstimator::new(config);
        let approve = estimator.estimate_approve_messages(3usize).await;
        assert_eq!(approve, 621);
    }

    #[tokio::test]
    async fn test_estimate_highload_wallet() {
        let config = GasEstimates {
            native_gas_refund: 1,
            native_gas_refund_storage_slippage: 1,
            execute: 1,
            execute_storage_slippage: 1,
            approve_fixed: 1,
            approve_fixed_storage_slippage: 1,
            approve_per_message: 1,
            approve_per_message_storage_slippage: 1,
            highload_wallet_per_action: 42
        };

        let estimator = TONGasEstimator::new(config);
        let approve = estimator.estimate_highload_wallet(3usize).await;
        assert_eq!(approve, 126);
    }


    #[tokio::test]
    async fn test_zero_values() {
        let config = GasEstimates {
            native_gas_refund: 0,
            native_gas_refund_storage_slippage: 0,
            execute: 0,
            execute_storage_slippage: 0,
            approve_fixed: 0,
            approve_fixed_storage_slippage: 0,
            approve_per_message: 0,
            approve_per_message_storage_slippage: 0,
            highload_wallet_per_action: 0
        };

        let estimator = TONGasEstimator::new(config);
        let refund = estimator.estimate_native_gas_refund().await;
        let execute = estimator.estimate_execute().await;
        let approve = estimator.estimate_approve_messages(5usize).await;
        let highload_wallet = estimator.estimate_highload_wallet(5usize).await;
        assert_eq!(refund, 0);
        assert_eq!(execute, 0);
        assert_eq!(approve, 0);
        assert_eq!(highload_wallet, 0);

    }
}
