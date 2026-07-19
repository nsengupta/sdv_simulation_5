//! Gateway library — exposes [`gateway_runtime`] (the [`TwinRuntimeBuilder`](gateway_runtime::TwinRuntimeBuilder))
//! so that the TUI Dashboard and other consumers can wire up a live twin without linking the Gateway binary.

pub mod cli;
pub mod gateway_runtime;
pub mod ingress;
pub mod transition_log;
pub mod twin_lifecycle;
