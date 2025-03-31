use std::sync::Arc;

use r2d2::{Pool, PooledConnection};
use redis::Commands;
use tracing::{debug, info, warn};

use crate::{
    error::DistributorError,
    gmp_api::GmpApi,
    queue::{Queue, QueueItem},
};

pub struct Distributor {
    redis_conn: PooledConnection<redis::Client>,
    last_task_id: Option<String>,
}

impl Distributor {
    pub fn new(redis_pool: Pool<redis::Client>) -> Self {
        let mut redis_conn = redis_pool
            .get()
            .expect("Cannot get redis connection from pool");

        let last_task_id = redis_conn.get("distributor:last_task_id").unwrap_or(None);
        if last_task_id.is_some() {
            info!(
                "Distributor: recovering last task id: {}",
                last_task_id.clone().unwrap()
            );
        }

        Self {
            redis_conn,
            last_task_id,
        }
    }

    async fn store_last_task_id(&mut self) -> Result<(), DistributorError> {
        if self.last_task_id.is_none() {
            return Ok(());
        }

        let _: () = self
            .redis_conn
            .set(
                "distributor:last_task_id",
                self.last_task_id.clone().unwrap(),
            )
            .map_err(|e| {
                DistributorError::GenericError(format!(
                    "Failed to store last_task_id to Redis: {}",
                    e
                ))
            })?;
        Ok(())
    }

    async fn work(&mut self, gmp_api: Arc<GmpApi>, queue: Arc<Queue>) {
        let tasks_res = gmp_api.get_tasks_action(self.last_task_id.clone()).await;
        match tasks_res {
            Ok(tasks) => {
                for task in tasks {
                    let task_item = &QueueItem::Task(task.clone());
                    info!("Publishing task: {:?}", task);
                    queue.publish(task_item.clone()).await;
                    let task_id = task.id();
                    self.last_task_id = Some(task_id);

                    if let Err(err) = self.store_last_task_id().await {
                        warn!("{:?}", err);
                    }
                }
            }
            Err(e) => {
                warn!("Error getting tasks: {:?}", e.to_string());
                debug!("Retrying in 2 seconds");
            }
        }
    }

    pub async fn run(&mut self, gmp_api: Arc<GmpApi>, queue: Arc<Queue>) {
        loop {
            info!("Distributor is alive.");
            self.work(gmp_api.clone(), queue.clone()).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
}
