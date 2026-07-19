//! Gateway — thin entrypoint that uses [`TwinRuntimeBuilder`] to bring up the live twin.
//!
//! Creates diagnostic and transition channels, attaches stdout observers, then calls
//! [`builder.run()`](gateway_runtime::TwinRuntimeBuilder::run).

use anyhow::Result;
use std::env;
use std::io::IsTerminal;
use tokio::sync::mpsc;

mod gateway_runtime;
mod ingress;
mod transition_log;

const VIRTUAL_CAR_IDENTITY: &str = "My-Opel-Corsa-1.4-GSi";

#[tokio::main]
async fn main() -> Result<()> {
    let print_transitions_only = env::args().any(|arg| arg == "--print-transitions-only");
    let trace_actuation_ingress = env::args().any(|arg| arg == "--trace-actuation-ingress");

    let mut builder = gateway_runtime::TwinRuntimeBuilder::new()
        .with_car_identity(VIRTUAL_CAR_IDENTITY)
        .with_can_interface(gateway_runtime::DEFAULT_CAN_INTERFACE)
        .with_ingress_console_log(true);

    if trace_actuation_ingress && !print_transitions_only {
        builder = builder.with_actuation_ingress_trace();
    }

    if print_transitions_only {
        // Ledger-only mode: wire transition channel to stdout, no diagnostic channel.
        let (trans_tx, trans_rx) = mpsc::channel::<common::facade::PublishedTransitionRecord>(256);
        let color = std::io::stdout().is_terminal();
        let _trans_log = transition_log::spawn_transition_log_task(trans_rx, color);
        builder = builder.with_transition_channel(trans_tx);

        eprintln!("[gateway] ledger-only mode (--print-transitions-only); colours={color}",);
    } else {
        // Normal mode: wire diagnostic channel to stdout.
        let (diag_tx, diag_rx) = mpsc::unbounded_channel();
        let _diag_obs = builder.with_stdout_diagnostic_observer(diag_rx);
        builder = builder.with_diagnostic_channel(diag_tx);
    }

    builder.run().await
}
