//! Compatibility shim — use [`crate::observation_records::transition`] instead.
//!
//! Will be removed once all call sites migrate to `observation_records`.

pub use crate::observation_records::transition::*;
