# Moved — process split and builder plan

Content from this plan is **merged into the phased roadmap**:

- **Target topology (Gateway vs Dashboard):** [`docs/ARCHITECTURE-OVERVIEW.md`](docs/ARCHITECTURE-OVERVIEW.md) §1
- **Process split implementation:** [`docs/PHASES.md`](docs/PHASES.md) **Phase 5**
- **TwinRuntimeBuilder / channel ownership:** still valid; see `crates/gateway/src/gateway_runtime.rs`

Zenoh/uProtocol for inter-process observation is **Phase 8** — not required for the initial Gateway/Dashboard split (Phase 5 uses file tail or simple IPC).

*Redirect stub — 2026-07-15*
