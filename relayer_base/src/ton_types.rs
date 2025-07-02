// This is probably not the right place for ton types. However, these types are our own construct
// and they don't make a lot of sense to belong in a separate, reusable project. 

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TONLogEvent {
    op_code: String,
    message_body: String
}
