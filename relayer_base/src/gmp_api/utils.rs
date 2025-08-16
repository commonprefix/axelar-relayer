use std::collections::HashSet;

use crate::gmp_api::gmp_types::Event;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fixtures;

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
}
