use relayer_base::gmp_api::gmp_types::RetryTask;

pub fn message_id_from_retry_task(task: RetryTask) -> Result<Option<String>, anyhow::Error> {
    match task {
        RetryTask::ReactToRetriablePoll(task) => {
            let message: xrpl_gateway::msg::ExecuteMsg =
                serde_json::from_str(&task.task.request_payload)?;
            if let xrpl_gateway::msg::ExecuteMsg::VerifyMessages(messages) = message {
                let tx_id = messages
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("No messages found in VerifyMessages payload"))?
                    .tx_id()
                    .tx_hash_as_hex_no_prefix();
                Ok(Some(tx_id.to_string()))
            } else {
                Err(anyhow::anyhow!("Unknown payload: {:?}", message))
            }
        }
        RetryTask::ReactToExpiredSigningSession(task) => {
            let message: xrpl_multisig_prover::msg::ExecuteMsg =
                serde_json::from_str(&task.task.request_payload)?;
            if let xrpl_multisig_prover::msg::ExecuteMsg::ConstructProof { cc_id, .. } = message {
                Ok(Some(cc_id.to_string()))
            } else if let xrpl_multisig_prover::msg::ExecuteMsg::TicketCreate = message {
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Unknown payload: {:?}", message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use relayer_base::gmp_api::gmp_types::{
        ReactToExpiredSigningSessionTask, ReactToRetriablePollTask, RetryTask,
    };
    use router_api::CrossChainId;

    use crate::utils::message_id_from_retry_task;

    #[test]
    fn test_message_id_from_retry_task_react_to_retriable_poll() {
        let valid_tasks_dir = "../relayer_base/testdata/gmp_tasks/valid_tasks";
        let json_str =
            fs::read_to_string(format!("{}/ReactToRetriablePollTask.json", valid_tasks_dir))
                .expect("Failed to read ReactToRetriablePollTask.json");
        let tasks: Vec<ReactToRetriablePollTask> = serde_json::from_str(&json_str)
            .expect("Failed to parse JSON into Vec<ReactToRetriablePollTask>");
        // test a specific valid task
        let task = tasks
            .clone()
            .into_iter()
            .nth(1)
            .expect("Missing second task");
        let maybe_message_id = message_id_from_retry_task(RetryTask::ReactToRetriablePoll(task));
        assert_eq!(
            maybe_message_id.unwrap(),
            Some("5fa140ff4b90c83df9fdfdc81595bd134f41d929694eedb15cf7fd1c511e8025".to_string())
        );

        // test a specific valid task which does not have the fields we need
        let task_err = tasks.into_iter().next().expect("Missing first task");
        let err = message_id_from_retry_task(RetryTask::ReactToRetriablePoll(task_err));
        assert!(err.is_err());
    }

    #[test]
    fn test_message_id_from_retry_task_react_to_expired_signing_session() {
        let valid_tasks_dir = "../relayer_base/testdata/gmp_tasks/valid_tasks";
        let json_str = fs::read_to_string(format!(
            "{}/ReactToExpiredSigningSessionTask.json",
            valid_tasks_dir
        ))
        .expect("Failed to read ReactToExpiredSigningSessionTask.json");
        let tasks: Vec<ReactToExpiredSigningSessionTask> = serde_json::from_str(&json_str)
            .expect("Failed to parse JSON into Vec<ReactToExpiredSigningSessionTask>");
        // test a specific valid task
        let task = tasks
            .clone()
            .into_iter()
            .nth(1)
            .expect("Missing second task");
        let maybe_message_id =
            message_id_from_retry_task(RetryTask::ReactToExpiredSigningSession(task));
        assert!(maybe_message_id.is_ok());
        let actual_source_chain = "axelar";
        // changed to see that the test CI fails
        let actual_message_id =
            "0x054e170d88e181b39f638cd5da6f3c76d1a5c4f0945a4540ffddc5e13965444b-150693963";
        let actual_cc_id = CrossChainId::new(actual_source_chain, actual_message_id).unwrap();

        assert_eq!(maybe_message_id.unwrap(), Some(actual_cc_id.to_string()));

        // test a specific valid task which does not have the fields we need
        let task_err = tasks.into_iter().next().expect("Missing first task");
        let err = message_id_from_retry_task(RetryTask::ReactToExpiredSigningSession(task_err));
        assert!(err.is_err());
    }
}
