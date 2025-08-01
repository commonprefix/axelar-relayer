/*!
Parses a wallet-wrapped RelayerExecuteMessage. We only care about the message id.


# Usage Example

```rust,no_run
use ton::boc::relayer_execute_wrapped::RelayerExecuteWrappedMessage;

let boc = "te6cck...";

match RelayerExecuteWrappedMessage::from_boc_b64(boc) {
    Ok(msg) => {
        // msg.message_id
    },
    Err(e) => println!("Failed to parse message: {:?}", e),
}
```

*/

use crate::boc::cell_to::CellTo;
use crate::error::BocError;
use crate::error::BocError::BocParsingError;
use serde::{Deserialize, Serialize};
use tonlib_core::cell::{Cell, CellParser};
use tonlib_core::tlb_types::tlb::TLB;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RelayerExecuteWrappedMessage {
    pub(crate) message_id: String,
    pub(crate) source_chain: String,
}

impl RelayerExecuteWrappedMessage {
    pub fn from_boc_b64(boc: &str) -> Result<RelayerExecuteWrappedMessage, BocError> {
        let cell = Cell::from_boc_b64(boc).map_err(|err| BocParsingError(err.to_string()))?;

        let mut outer_parser = cell.parser();
        let cell = outer_parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();
        let _ = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let cell = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();
        let cell = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();
        let cell = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?;
        let mut parser: CellParser = cell.parser();
        let message_id = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()
            .map_err(|err| BocParsingError(err.to_string()))?;

        let source_chain = parser
            .next_reference()
            .map_err(|err| BocParsingError(err.to_string()))?
            .cell_to_string()
            .map_err(|err| BocParsingError(err.to_string()))?;

        Ok(Self {
            message_id,
            source_chain,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::boc::relayer_execute_wrapped::RelayerExecuteWrappedMessage;

    #[test]
    fn test_from_boc_b64() {
        let boc = "te6cckECLgEADt0AARiuQuWkAAAAAAAAAAUBAgoOw8htAgIDAAABaCIAAAylVschFfb6H/c6NrbJTptO3VkqNFNOqJuDdTS8Wx0gk9HMAAAAAAAAAAAAAAAAAAEEAUsAAAAIgBIHqwhg5lg4ES2+GWhwn4EVgGvmj7MoTr6OJXwhB8BysAUEAAYLBwgAhDB4ODlmMzI1MmZiOWFkNzAwM2MyNTQ3MTY4NWY0OGQxNGM4NDJhMjkxMGI5Mzg2ZDM1Zjg1OTY5NGJhYmY3YjFjZgCEMDplMWU2MzNlYjcwMWIxMThiNDQyOTc3MTZjZWU3MDY5ZWU4NDdiNTZkYjg4YzQ5N2VmZWE2ODFlZDE0YjJkMmM3A4CIsmyAnqYuP9QkpkQ/rH/UulMugyeDeOANdz4XWXr/xQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAmJaACQoLAcAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAyuTG9yZW0gaXBzdW0gZG9sb3Igc2l0IGFtZXQsIGNvbnMMAEDtIt80IZriYDn9l32OQZrhTXixkunbXc+jWXiZCWRw0QAIdG9uMgHAZWN0ZXR1ciBhZGlwaXNjaW5nIGVsaXQuIEluIGJsYW5kaXQsIGFudGUgaW4gZGlnbmlzc2ltIHVsdHJpY2llcywgbGlndWxhIHB1cnVzIG1vbGVzdGllIHNhcGllbiwgDQHAZXVpc21vZCBzYWdpdHRpcyBuaXNpIGxlY3R1cyB1dCBhbnRlLiBJbnRlZ2VyIGltcGVyZGlldCBpcHN1bSBlcm9zLCBmZXVnaWF0IGNvbmRpbWVudHVtIGFudGUgZ3JhDgHAdmlkYSBldC4gU3VzcGVuZGlzc2UgdGVtcHVzIGxvcmVtIHNpdCBhbWV0IG1hc3NhIGVmZmljaXR1ciBzb2xsaWNpdHVkaW4uIE5hbSBvZGlvIGR1aSwgY29udmFsbGlzDwHAIHZpdGFlIG1hdXJpcyBxdWlzLCBmYXVjaWJ1cyB1bGxhbWNvcnBlciBzYXBpZW4uIERvbmVjIG1vbGVzdGllIGNvbnZhbGxpcyBlbGVtZW50dW0uIFF1aXNxdWUgdGluEAHAY2lkdW50IHRpbmNpZHVudCBuZXF1ZSwgbm9uIHBsYWNlcmF0IG5lcXVlIHBvc3VlcmUgYXQuIE51bGxhbSBjb25zZXF1YXQgdml2ZXJyYSBwb3J0YS4gRG9uZWMgZXQgEQHAbWFsZXN1YWRhIHF1YW0sIHNlZCBjb252YWxsaXMgdmVsaXQuICBVdCBuaXNsIHR1cnBpcywgdGluY2lkdW50IHZlbCBjb21tb2RvIHV0LCBzb2RhbGVzIGV1IG5pc2kuEgHAIE1hdXJpcyBwaGFyZXRyYSBkdWkgYSBzZW0gZmV1Z2lhdCBwb3N1ZXJlLiBQZWxsZW50ZXNxdWUgc2VkIHBvc3VlcmUgZHVpLiBQZWxsZW50ZXNxdWUgbGFjaW5pYSBmEwHAZWxpcyBldCBkaWFtIHZlbmVuYXRpcyBzYWdpdHRpcy4gTWF1cmlzIHZlbGl0IHB1cnVzLCBmaW5pYnVzIG5vbiBuaXNsIGF0LCBzdXNjaXBpdCBpYWN1bGlzIGFyY3UuFAHAIE1vcmJpIGNvbnNlY3RldHVyIHBlbGxlbnRlc3F1ZSBsYWN1cywgdmVsIGJpYmVuZHVtIGp1c3RvIGZyaW5naWxsYSB1dC4gU2VkIHNhZ2l0dGlzIGVsZWlmZW5kIG51FQHAbGxhLCBhYyBpYWN1bGlzIHR1cnBpcyB0cmlzdGlxdWUgZWdldC4gQWVuZWFuIHV0IHBvcnR0aXRvciBuaXNsLCBjb21tb2RvIGVsZWlmZW5kIGxvcmVtLiBJbnRlZ2VyFgHAIGVnZXQgcHVydXMgdmVuZW5hdGlzIG1pIGVmZmljaXR1ciBtb2xlc3RpZSBpZCBsb2JvcnRpcyBuZXF1ZS4gU2VkIHF1aXMgcmhvbmN1cyBwdXJ1cy4gVml2YW11cyB2FwHAaXZlcnJhIG51bGxhIHNlZCBkaWFtIGZyaW5naWxsYSwgdml0YWUgcnV0cnVtIGxpYmVybyB2ZXN0aWJ1bHVtLiBGdXNjZSBzZW0gZW5pbSwgcnV0cnVtIG5lYyBlbmltGAHAIHBlbGxlbnRlc3F1ZSwgZWxlbWVudHVtIG1hdHRpcyBzZW0uIE51bGxhbSB1bGxhbWNvcnBlciBhY2N1bXNhbiBpcHN1bSwgc2VkIG1vbGVzdGllIG5pYmggdmVoaWN1GQHAbGEgc2l0IGFtZXQuIE1hdXJpcyBmZXJtZW50dW0gYWMgZXN0IHZpdGFlIGF1Y3Rvci4gQWVuZWFuIGZlcm1lbnR1bSBjb252YWxsaXMgbmVxdWUsIHNlZCB2ZWhpY3VsGgHAYSBlc3QgZGlnbmlzc2ltIGEuICBJbiBsYWNpbmlhIG5pYmggbmlzaSwgZXUgZWdlc3RhcyBtYXVyaXMgdGVtcHVzIGEuIFBoYXNlbGx1cyBub24gZWxlbWVudHVtIGRvGwHAbG9yLiBJbnRlZ2VyIGdyYXZpZGEgcGhhcmV0cmEgZmF1Y2lidXMuIFBlbGxlbnRlc3F1ZSBtb2xsaXMgZG9sb3IgcXVpcyBhcmN1IHZpdmVycmEsIGF0IHBoYXJldHJhHAHAIGxlY3R1cyBzYWdpdHRpcy4gSW50ZXJkdW0gZXQgbWFsZXN1YWRhIGZhbWVzIGFjIGFudGUgaXBzdW0gcHJpbWlzIGluIGZhdWNpYnVzLiBQcmFlc2VudCBzdXNjaXBpHQHAdCBhdWd1ZSBhYyBtYWduYSBkYXBpYnVzIG9ybmFyZS4gRHVpcyBmcmluZ2lsbGEgaWFjdWxpcyBmZXJtZW50dW0uIEFlbmVhbiBpZCByaXN1cyBkdWkuIERvbmVjIHNlHgHAZCBtb2xlc3RpZSBkdWkuIE51bmMgc2VtIG1hc3NhLCBpbnRlcmR1bSB1dCBjb25ndWUgdmVsLCBoZW5kcmVyaXQgdmVzdGlidWx1bSBsaWJlcm8uIFN1c3BlbmRpc3NlHwHAIHN1c2NpcGl0IGF1Y3RvciBlZmZpY2l0dXIuICBQcmFlc2VudCB1dCBmYXVjaWJ1cyBudW5jLCBhdCB0aW5jaWR1bnQgZXJvcy4gTW9yYmkgZXN0IG1hc3NhLCBmZXJtIAHAZW50dW0gc2l0IGFtZXQgbmVxdWUgdmVsLCBpYWN1bGlzIHZvbHV0cGF0IHB1cnVzLiBJbnRlcmR1bSBldCBtYWxlc3VhZGEgZmFtZXMgYWMgYW50ZSBpcHN1bSBwcmltIQHAaXMgaW4gZmF1Y2lidXMuIERvbmVjIG5lcXVlIG1pLCBkaWN0dW0gZXUgaGVuZHJlcml0IHZpdGFlLCBhdWN0b3IgZXQgdGVsbHVzLiBOYW0gYXVjdG9yIGxpZ3VsYSBpIgHAbiBvcmNpIHNvbGxpY2l0dWRpbiBvcm5hcmUuIFNlZCBuZWMgbG9yZW0gZnJpbmdpbGxhLCBwb3J0dGl0b3IgbG9yZW0gdXQsIGNvbnNlcXVhdCBlbGl0LiBOdWxsYW0gIwHAc29sbGljaXR1ZGluIHB1cnVzIG1pLCBpbiB1bHRyaWNlcyBkdWkgbW9sbGlzIHNlZC4gSW50ZWdlciBsYWNpbmlhIG5pYmggZXQgZmluaWJ1cyBlZmZpY2l0dXIuIENyJAHAYXMgZmV1Z2lhdCB2ZXN0aWJ1bHVtIHNhcGllbiBmZXJtZW50dW0gY3Vyc3VzLiBQZWxsZW50ZXNxdWUgdXQgc2FwaWVuIGxpZ3VsYS4gQ3VyYWJpdHVyIGVsaXQgZW5pJQHAbSwgcG9ydGEgcXVpcyB1cm5hIGFjLCBwZWxsZW50ZXNxdWUgdGVtcHVzIG5pc2kuICBJbnRlZ2VyIGNvbnNlY3RldHVyIGxvYm9ydGlzIGZhdWNpYnVzLiBOdWxsYSBzJgHAZWQgZG9sb3IgbGVvLiBQcmFlc2VudCBhbGlxdWFtIGVyb3Mgdml0YWUgcG9ydGEgbGFjaW5pYS4gRG9uZWMgZXQgbG9yZW0gdGluY2lkdW50LCB2aXZlcnJhIG51bmMgJwHAaW4sIG1hbGVzdWFkYSBuaXNpLiBQZWxsZW50ZXNxdWUgbmVxdWUgbWF1cmlzLCBiaWJlbmR1bSBuZWMgcHVsdmluYXIgbm9uLCBldWlzbW9kIHV0IGxlY3R1cy4gVmVzKAHAdGlidWx1bSBxdWlzIGJpYmVuZHVtIHRlbGx1cy4gVXQgbHVjdHVzIG1hdXJpcyBmZXJtZW50dW0gcmhvbmN1cyBjb25kaW1lbnR1bS4gQWVuZWFuIHNlZCBsYWN1cyBxKQHAdWlzIGxlY3R1cyBwdWx2aW5hciBmaW5pYnVzLiBNYWVjZW5hcyBhIHBvcnR0aXRvciBsZW8sIHZlbCB0ZW1wb3IgdGVsbHVzLiBWaXZhbXVzIGxlY3R1cyB0ZWxsdXMsKgHAIHNvbGxpY2l0dWRpbiBhIG9kaW8gbW9sZXN0aWUsIHZlbmVuYXRpcyBpYWN1bGlzIG1ldHVzLiBTZWQgcXVpcyBsZWN0dXMgbnVsbGEuIFNlZCBqdXN0byBtYXNzYSwgKwHAc3VzY2lwaXQgdXQgZWxlaWZlbmQgbmVjLCBncmF2aWRhIHV0IG5pYmguIE1hdXJpcyBxdWlzIG1ldHVzIHVybmEuIE51bGxhbSBhdCBtYXVyaXMgc2VkIG1pIGltcGVyLAHAZGlldCBlbGVpZmVuZCB1bHRyaWNlcyBzZWQgb2Rpby4gTmFtIGxhb3JlZXQgbGVvIGluIGFyY3UgZ3JhdmlkYSwgZWdldCBtYXhpbXVzIGlwc3VtIGxhY2luaWEuIE1hLQCAZWNlbmFzIGNvbnNlcXVhdCBuZXF1ZSBhYyBwZWxsZW50ZXNxdWUgdGVtcG9yLgAAAAAAAAAAAAAAAAAAAAAAAMZ+Rtk=";
        let res = RelayerExecuteWrappedMessage::from_boc_b64(boc);
        let res = res.unwrap();
        assert_eq!(
            res.message_id,
            "0x89f3252fb9ad7003c25471685f48d14c842a2910b9386d35f859694babf7b1cf"
        );
        assert_eq!(res.source_chain, "ton2");
    }
}
