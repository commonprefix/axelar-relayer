use std::future::Future;
use crate::config::GasEstimates;

pub struct TONGasEstimator {
    config: GasEstimates
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
}

impl GasEstimator for TONGasEstimator {
    async fn estimate_native_gas_refund(&self) -> u64 {
        self.config.native_gas_refund + self.config.native_gas_refund_storage_slippage
    }

    async fn estimate_execute(&self) -> u64 {
        self.config.execute + self.config.execute_storage_slippage
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
        };

        let estimator = TONGasEstimator::new(config);
        let execute = estimator.estimate_execute().await;
        assert_eq!(execute, 250);
    }

    #[tokio::test]
    async fn test_zero_values() {
        let config = GasEstimates {
            native_gas_refund: 0,
            native_gas_refund_storage_slippage: 0,
            execute: 0,
            execute_storage_slippage: 0,
        };

        let estimator = TONGasEstimator::new(config);
        let refund = estimator.estimate_native_gas_refund().await;
        let execute = estimator.estimate_execute().await;

        assert_eq!(refund, 0);
        assert_eq!(execute, 0);
    }
}
