use std::collections::HashSet;

use crate::gmp_api::gmp_types::{Event, Task};

pub fn extract_message_ids_from_task(task: &Task) -> Vec<String> {
    let mut message_ids = HashSet::new();

    match task {
        Task::Execute(t) => {
            message_ids.insert(t.task.message.message_id.clone());
        }
        Task::Verify(t) => {
            message_ids.insert(t.task.message.message_id.clone());
        }
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

    message_ids.into_iter().collect()
}

pub fn extract_message_ids_and_event_types_from_events(
    events: &[Event],
) -> (Vec<String>, Vec<String>) {
    let mut message_ids = HashSet::new();
    let mut event_types = HashSet::new();

    for event in events {
        // Extract event type
        match event {
            Event::Call { .. } => {
                event_types.insert("Call".to_string());
            }
            Event::GasRefunded { .. } => {
                event_types.insert("GasRefunded".to_string());
            }
            Event::GasCredit { .. } => {
                event_types.insert("GasCredit".to_string());
            }
            Event::MessageApproved { .. } => {
                event_types.insert("MessageApproved".to_string());
            }
            Event::MessageExecuted { .. } => {
                event_types.insert("MessageExecuted".to_string());
            }
            Event::CannotExecuteMessageV2 { .. } => {
                event_types.insert("CannotExecuteMessageV2".to_string());
            }
            Event::ITSInterchainTransfer { .. } => {
                event_types.insert("ITSInterchainTransfer".to_string());
            }
        }

        // Extract message ID
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

    // Convert HashSets to Vecs
    let message_ids_vec: Vec<String> = message_ids.into_iter().collect();

    let mut event_types_vec: Vec<String> = event_types.into_iter().collect();
    event_types_vec.sort(); // Sort event types alphabetically

    (message_ids_vec, event_types_vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gmp_api::gmp_types::{ScopedMessage, TaskMetadata};
    use crate::test_utils::fixtures;

    #[test]
    fn test_extract_message_ids_from_task_execute() {
        let task = fixtures::execute_task();
        let message_ids = extract_message_ids_from_task(&task);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_task_verify() {
        let task = fixtures::verify_task();
        let message_ids = extract_message_ids_from_task(&task);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_task_construct_proof() {
        let task = fixtures::construct_proof_task();
        let message_ids = extract_message_ids_from_task(&task);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_task_refund() {
        let task = fixtures::refund_task();
        let message_ids = extract_message_ids_from_task(&task);
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));
    }

    #[test]
    fn test_extract_message_ids_from_task_no_message_id() {
        let task = fixtures::gateway_tx_task();
        let message_ids = extract_message_ids_from_task(&task);
        assert_eq!(message_ids.len(), 0);
    }

    #[test]
    fn test_extract_message_ids_from_task_with_scoped_messages() {
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

        let message_ids = extract_message_ids_from_task(&task);
        assert_eq!(message_ids.len(), 3);
        assert!(message_ids.contains(&"message123".to_string()));
        assert!(message_ids.contains(&"scoped_message_1".to_string()));
        assert!(message_ids.contains(&"scoped_message_2".to_string()));
    }

    #[test]
    fn test_extract_message_ids_and_event_types_from_events_empty() {
        let events: Vec<Event> = vec![];
        let (message_ids, event_types) = extract_message_ids_and_event_types_from_events(&events);
        assert!(message_ids.is_empty());
        assert!(event_types.is_empty());
    }

    #[test]
    fn test_extract_message_ids_and_event_types_from_events_single_event() {
        let events = vec![fixtures::gas_refunded_event()];
        let (message_ids, event_types) = extract_message_ids_and_event_types_from_events(&events);

        // Check message IDs
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));

        // Check event types
        assert_eq!(event_types.len(), 1);
        assert_eq!(event_types[0], "GasRefunded");
    }

    #[test]
    fn test_extract_message_ids_and_event_types_from_events_multiple_events() {
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

        let (message_ids, event_types) = extract_message_ids_and_event_types_from_events(&events);

        // Check message IDs
        assert_eq!(message_ids.len(), 2);
        assert!(message_ids.contains(&"message123".to_string()));
        assert!(message_ids.contains(&"different_id".to_string()));

        // Check event types
        assert_eq!(event_types.len(), 7);

        // Verify alphabetical sorting
        let expected_types = vec![
            "Call".to_string(),
            "CannotExecuteMessageV2".to_string(),
            "GasCredit".to_string(),
            "GasRefunded".to_string(),
            "ITSInterchainTransfer".to_string(),
            "MessageApproved".to_string(),
            "MessageExecuted".to_string(),
        ];
        assert_eq!(event_types, expected_types);
    }

    #[test]
    fn test_extract_message_ids_and_event_types_from_events_duplicates() {
        let events = vec![
            fixtures::cannot_execute_message_v2_event(),
            fixtures::gas_refunded_event(),
            fixtures::gas_refunded_event(),
            fixtures::call_event(),
            fixtures::call_event(),
        ];

        let (message_ids, event_types) = extract_message_ids_and_event_types_from_events(&events);

        // Message ids should be deduplicated
        assert_eq!(message_ids.len(), 1);
        assert!(message_ids.contains(&"message123".to_string()));

        // Event types should be deduplicated and sorted alphabetically
        assert_eq!(event_types.len(), 3);

        let expected_types = vec![
            "Call".to_string(),
            "CannotExecuteMessageV2".to_string(),
            "GasRefunded".to_string(),
        ];
        assert_eq!(event_types, expected_types);
    }
}
