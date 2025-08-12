use std::sync::Arc;
use tracing::{info, info_span, warn, Instrument};

use crate::gmp_api::GmpApiTrait;
use crate::utils::ThreadSafe;
use crate::{
    database::Database,
    error::DistributorError,
    gmp_api::gmp_types::TaskKind,
    queue::{Queue, QueueItem},
};

#[derive(Clone)]
pub struct RecoverySettings {
    pub from_task_id: Option<String>,
    pub to_task_id: String,
    pub tasks_filter: Option<Vec<TaskKind>>,
}

pub struct Distributor<DB: Database, G: GmpApiTrait + ThreadSafe> {
    db: DB,
    last_task_id: Option<String>,
    context: String,
    recovery_settings: Option<RecoverySettings>,
    gmp_api: Arc<G>,
    refunds_enabled: bool,
    supported_includer_tasks: Vec<TaskKind>,
    supported_ingestor_tasks: Vec<TaskKind>,
}

impl<DB, G> Distributor<DB, G>
where
    DB: Database,
    G: GmpApiTrait + ThreadSafe,
{
    pub async fn new(db: DB, context: String, gmp_api: Arc<G>, refunds_enabled: bool) -> Self {
        let last_task_id = db
            .get_latest_task_id(gmp_api.get_chain(), &context)
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
            // Sane default as it's the minimum required for the relayer to work
            supported_includer_tasks: vec![TaskKind::Refund, TaskKind::GatewayTx],
            supported_ingestor_tasks: vec![
                TaskKind::Verify,
                TaskKind::ConstructProof,
                TaskKind::ReactToWasmEvent,
                TaskKind::ReactToRetriablePoll,
                TaskKind::ReactToExpiredSigningSession,
            ],
        }
    }

    pub async fn new_with_recovery_settings(
        db: DB,
        context: String,
        gmp_api: Arc<G>,
        recovery_settings: RecoverySettings,
        refunds_enabled: bool,
    ) -> Result<Self, DistributorError> {
        let mut distributor = Self::new(db, context, gmp_api, refunds_enabled).await;
        distributor.recovery_settings = Some(recovery_settings.clone());
        distributor.last_task_id = recovery_settings.from_task_id;
        distributor.store_last_task_id().await?;
        Ok(distributor)
    }

    #[tracing::instrument(skip(self))]
    pub async fn store_last_task_id(&mut self) -> Result<(), DistributorError> {
        if let Some(task_id) = &self.last_task_id {
            self.db
                .store_latest_task_id(self.gmp_api.get_chain(), &self.context, task_id)
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
            let span = info_span!("received_task", task = format!("{task:?}"));

            processed_task_ids.push(task_id.clone());
            self.last_task_id = Some(task_id);

            if let Err(err) = self.store_last_task_id().instrument(span.clone()).await {
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

            let queue = if self.supported_includer_tasks.contains(&task.kind()) {
                info!("Publishing task to includer queue: {:?}", task);
                Arc::<Queue>::clone(&includer_queue)
            } else if self.supported_ingestor_tasks.contains(&task.kind()) {
                info!("Publishing task to ingestor queue: {:?}", task);
                Arc::<Queue>::clone(&ingestor_queue)
            } else {
                warn!("Dropping unsupported task: {:?}", task);
                continue;
            };

            queue.publish(task_item.clone()).instrument(span).await;
        }
        Ok(processed_task_ids)
    }

    pub async fn run(&mut self, includer_queue: Arc<Queue>, ingestor_queue: Arc<Queue>) {
        loop {
            info!("Distributor is alive.");
            let work_res = self
                .work(
                    Arc::clone(&includer_queue),
                    Arc::clone(&ingestor_queue),
                    None,
                )
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
                    Arc::clone(&includer_queue),
                    Arc::clone(&ingestor_queue),
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

    pub fn set_supported_includer_tasks(&mut self, tasks: Vec<TaskKind>) {
        self.supported_includer_tasks = tasks;
    }

    pub fn set_supported_ingestor_tasks(&mut self, tasks: Vec<TaskKind>) {
        self.supported_ingestor_tasks = tasks;
    }
}
