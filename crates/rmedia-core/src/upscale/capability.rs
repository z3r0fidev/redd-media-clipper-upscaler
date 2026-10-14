//! Machine capability record: GPU listing, fingerprint, self-tests, demotions (research R8).

use serde::Serialize;

/// The machine's cached capability record (`<state>/capability.json`). Filled in by US3; until
/// then the planner treats a missing record as "CPU tier only".
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CapabilityRecord {}
