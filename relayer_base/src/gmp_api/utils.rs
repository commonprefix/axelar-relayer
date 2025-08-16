use std::collections::HashSet;

use crate::gmp_api::gmp_types::{Event, Task, ScopedMessage};

pub fn extract_message_ids_from_events(events: &[Event]) -> Vec<String> {
    let mut message_ids = HashSet::new();

    for event in events {
        match event {
            Event::Call { message, .. } | Event::MessageApproved { message, .. } => {
                message_ids.insert(message.message_id.clone());
            }
            Event::GasRefunded { message_id, .. }
            | Event::GasCredit { message_id, .. }
            | Event::MessageExecuted { message_id, .. }
            | Event::CannotExecuteMessageV2 { message_id, .. }
            | Event::ITSInterchainTransfer { message_id, .. } => {
                message_ids.insert(message_id.clone());
            }
        }
    }

    message_ids.into_iter().collect()
}

pub fn extract_message_ids_from_tasks(tasks: &[Task]) -> Vec<String> {
    let mut message_ids = HashSet::new();

    for task in tasks {
        match task {
            Task::Execute(t) => {
                message_ids.insert(t.task.message.message_id.clone());
            }
            Task::Verify(t) => {
                message_ids.insert(t.task.message.message_id.clone());
            },
            Task::ConstructProof(t) => {
                message_ids.insert(t.task.message.message_id.clone());
            }
            Task::Refund(t) => {
                message_ids.insert(t.task.message.message_id.clone());
            }
            _ => {}
        }

        // Extract message IDs from scoped_messages in task metadata if present
        if let Some(meta) = match task {
            Task::Execute(t) => &t.common.meta,
            Task::Verify(t) => &t.common.meta,
            Task::GatewayTx(t) => &t.common.meta,
            Task::ConstructProof(t) => &t.common.meta,
            Task::ReactToWasmEvent(t) => &t.common.meta,
            Task::Refund(t) => &t.common.meta,
            Task::ReactToExpiredSigningSession(t) => &t.common.meta,
            Task::ReactToRetriablePoll(t) => &t.common.meta,
            Task::Unknown(t) => &t.common.meta,
        } {
            if let Some(scoped_messages) = &meta.scoped_messages {
                for scoped_message in scoped_messages {
                    message_ids.insert(scoped_message.message_id.clone());
                }
            }
        }
    }

    message_ids.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures;
    use crate::gmp_api::gmp_types::{TaskMetadata, ScopedMessage};
    use std::collections::BTreeMap;

    #[test]
    fn test_extract_message_ids_from_events_empty() {
        let events: Vec<Event> = vec![];
        let message_ids = extract_message_ids_from_events(&events);
        assert!(message_ids.is_empty());
    }

    #[test]
    fn test_extract_message_ids_from_events_single_event() {
        let events = vec![fixtures::gas_refunded_event()];
        let message_ids = extract_message_ids_from_events(&events);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_events_duplicate_ids() {
        // Both events have the same message_id "message123"
        let events = vec![fixtures::gas_refunded_event(), fixtures::gas_credit_event()];
        let message_ids = extract_message_ids_from_events(&events);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_events_different_event_types() {
        let mut events = vec![
            fixtures::call_event(),                      // message.message_id
            fixtures::message_approved_event(),          // message.message_id
            fixtures::gas_refunded_event(),              // message_id
            fixtures::gas_credit_event(),                // message_id
            fixtures::message_executed_event(),          // message_id
            fixtures::cannot_execute_message_v2_event(), // message_id
            fixtures::its_interchain_transfer_event(),   // message_id
        ];

        if let Event::Call {
            ref mut message, ..
        } = events[0]
        {
            message.message_id = "different_id".to_string();
        }

        let message_ids = extract_message_ids_from_events(&events);
        assert_eq!(message_ids.len(), 2);
        assert!(message_ids.contains(&"message123".to_string()));
        assert!(message_ids.contains(&"different_id".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_tasks_empty() {
        let tasks: Vec<Task> = vec![];
        let message_ids = extract_message_ids_from_tasks(&tasks);
        assert!(message_ids.is_empty());
    }

    #[test]
    fn test_extract_message_ids_from_tasks_single_task() {
        let tasks = vec![fixtures::execute_task()];
        let message_ids = extract_message_ids_from_tasks(&tasks);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_tasks_duplicate_ids() {
        // Both tasks have the same message_id "message123"
        let tasks = vec![
            fixtures::execute_task(),
            fixtures::verify_task(),
        ];
        let message_ids = extract_message_ids_from_tasks(&tasks);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_tasks_different_task_types() {
        let tasks = vec![
            fixtures::execute_task(),         // task.message.message_id
            fixtures::verify_task(),          // task.message.message_id
            fixtures::construct_proof_task(), // task.message.message_id
            fixtures::refund_task(),          // task.message.message_id
            fixtures::gateway_tx_task(),      // no message_id
            fixtures::react_to_wasm_event_task(), // no message_id
            fixtures::react_to_expired_signing_session_task(), // no message_id
            fixtures::react_to_retriable_poll_task(), // no message_id
            fixtures::unknown_task(),         // no message_id
        ];

        let message_ids = extract_message_ids_from_tasks(&tasks);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_tasks_with_scoped_messages() {
        let mut task = fixtures::execute_task();

        if let Task::Execute(ref mut t) = task {
            let scoped_messages = vec![
                ScopedMessage {
                    message_id: "scoped_message_1".to_string(),
                    source_chain: "chain1".to_string(),
                },
                ScopedMessage {
                    message_id: "scoped_message_2".to_string(),
                    source_chain: "chain2".to_string(),
                },
            ];

            t.common.meta = Some(TaskMetadata {
                tx_id: None,
                from_address: None,
                finalized: None,
                source_context: None,
                scoped_messages: Some(scoped_messages),
            });
        }

        let tasks = vec![task];
        let message_ids = extract_message_ids_from_tasks(&tasks);
        assert_eq!(message_ids.len(), 3);
        assert!(message_ids.contains(&"message123".to_string()));
        assert!(message_ids.contains(&"scoped_message_1".to_string()));
        assert!(message_ids.contains(&"scoped_message_2".to_string()));
    }
}
