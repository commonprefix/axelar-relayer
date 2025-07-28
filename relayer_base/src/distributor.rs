use std::sync::Arc;
use tracing::{info, warn};

use crate::{
    database::Database,
    error::DistributorError,
    gmp_api::{gmp_types::TaskKind, GmpApi},
    queue::{Queue, QueueItem},
};

#[derive(Clone)]
pub struct RecoverySettings {
    pub from_task_id: Option<String>,
    pub to_task_id: String,
    pub tasks_filter: Option<Vec<TaskKind>>,
}

pub struct Distributor<DB: Database> {
    db: DB,
    last_task_id: Option<String>,
    context: String,
    recovery_settings: Option<RecoverySettings>,
    gmp_api: Arc<GmpApi>,
    refunds_enabled: bool,
}

impl<DB: Database> Distributor<DB> {
    pub async fn new(db: DB, context: String, gmp_api: Arc<GmpApi>, refunds_enabled: bool) -> Self {
        let last_task_id = db
            .get_latest_task_id(&gmp_api.chain, &context)
            .await
            .expect("Failed to get latest task id");

        info!(
            "Distributor: recovering last task id from {}: {:?}",
            context, last_task_id
        );

        Self {
            db,
            last_task_id,
            context,
            recovery_settings: None,
            gmp_api,
            refunds_enabled,
        }
    }

    pub async fn new_with_recovery_settings(
        db: DB,
        context: String,
        gmp_api: Arc<GmpApi>,
        recovery_settings: RecoverySettings,
        refunds_enabled: bool,
    ) -> Result<Self, DistributorError> {
        let mut distributor = Self::new(db, context, gmp_api, refunds_enabled).await;
        distributor.recovery_settings = Some(recovery_settings.clone());
        distributor.last_task_id = recovery_settings.from_task_id;
        distributor.store_last_task_id().await?;
        Ok(distributor)
    }

    pub async fn store_last_task_id(&mut self) -> Result<(), DistributorError> {
        if let Some(task_id) = &self.last_task_id {
            self.db
                .store_latest_task_id(&self.gmp_api.chain, &self.context, task_id)
                .await
                .map_err(|e| {
                    DistributorError::GenericError(format!("Failed to store last_task_id: {}", e))
                })?;
        }
        Ok(())
    }

    async fn work(
        &mut self,
        includer_queue: Arc<Queue>,
        ingestor_queue: Arc<Queue>,
        tasks_filter: Option<Vec<TaskKind>>,
    ) -> Result<Vec<String>, DistributorError> {
        let mut processed_task_ids = Vec::new();
        let tasks = self
            .gmp_api
            .get_tasks_action(self.last_task_id.clone())
            .await
            .map_err(|e| DistributorError::GenericError(format!("Failed to get tasks: {}", e)))?;
        for task in tasks {
            let task_id = task.id();
            processed_task_ids.push(task_id.clone());
            self.last_task_id = Some(task_id);

            if let Err(err) = self.store_last_task_id().await {
                warn!("{:?}", err);
            }
            if let Some(tasks_filter) = &tasks_filter {
                if !tasks_filter.contains(&task.kind()) {
                    continue;
                }
            }

            if task.kind() == TaskKind::Refund && !self.refunds_enabled {
                continue;
            }

            let task_item = &QueueItem::Task(Box::new(task.clone()));
            info!("Publishing task: {:?}", task);
            let queue = match task.kind() {
                TaskKind::Refund | TaskKind::GatewayTx => includer_queue.clone(),
                TaskKind::Verify
                | TaskKind::ConstructProof
                | TaskKind::ReactToWasmEvent
                | TaskKind::ReactToRetriablePoll
                | TaskKind::ReactToExpiredSigningSession => ingestor_queue.clone(),
                TaskKind::Unknown | TaskKind::Execute => {
                    warn!("Dropping unsupported task: {:?}", task);
                    continue;
                }
            };
            queue.publish(task_item.clone()).await;
        }
        Ok(processed_task_ids)
    }

    pub async fn run(&mut self, includer_queue: Arc<Queue>, ingestor_queue: Arc<Queue>) {
        loop {
            info!("Distributor is alive.");
            let work_res = self
                .work(includer_queue.clone(), ingestor_queue.clone(), None)
                .await;
            if let Err(err) = work_res {
                warn!("{:?}\nRetrying in 2 seconds", err);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }

    pub async fn run_recovery(&mut self, includer_queue: Arc<Queue>, ingestor_queue: Arc<Queue>) {
        let recovery_settings = match &self.recovery_settings {
            Some(settings) => settings.clone(),
            None => {
                warn!("No recovery settings configured; skipping recovery loop.");
                return;
            }
        };
        loop {
            info!("Distributor is recovering.");
            let work_res = self
                .work(
                    includer_queue.clone(),
                    ingestor_queue.clone(),
                    recovery_settings.tasks_filter.clone(),
                )
                .await;
            if let Err(err) = work_res {
                warn!("{:?}\nRetrying in 2 seconds", err);
            } else if let Ok(task_ids) = work_res {
                if task_ids.contains(&recovery_settings.to_task_id) {
                    info!("Distributor has recovered.");
                    break;
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
}
