//! Compatibility shim — use [`crate::observation_records::transition::sink`] instead.
//!
//! Will be removed once all call sites migrate to `observation_records`.

pub use crate::observation_records::transition::PublishedTransitionRecord;
pub use crate::observation_records::transition::sink::{
    TokioMpscTransitionRecordSink, TransitionRecordSink, TransitionSinkError,
};
