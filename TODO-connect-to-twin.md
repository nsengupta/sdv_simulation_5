# Moved — process split complete (Phase 6)

Content from this plan is **merged into the phased roadmap**:

- **Target topology (Gateway vs Dashboard):** [`docs/ARCHITECTURE-OVERVIEW.md`](docs/ARCHITECTURE-OVERVIEW.md) §1
- **Process split (Done):** [`docs/PHASES.md`](docs/PHASES.md) **Phase 6**
- **Design / plan:** [`docs/superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md`](docs/superpowers/specs/2026-07-19-phase-6-gateway-dashboard-split-design.md),
  [`docs/superpowers/plans/2026-07-19-phase-6-gateway-dashboard-split.md`](docs/superpowers/plans/2026-07-19-phase-6-gateway-dashboard-split.md)
- **TwinRuntimeBuilder / channel ownership:** still valid; see `crates/gateway/src/gateway_runtime.rs`

Live observation today uses **UDS** under `<cwd>/tmp/` via detachable `LiveSink` / `LiveSource`.
Zenoh/uProtocol is **Phase 9** — not required for the Gateway/Dashboard split.

*Redirect stub — updated 2026-07-19*
