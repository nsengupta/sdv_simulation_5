# Design notes: pyramid layers

**Purpose:** the canonical L0–L6 layer list for this workspace, and the dependency rules that
keep the pyramid one-way. `crates/common/src/facade.rs` and other modules reference this document
as the source of truth for layering; this file is that document.

## The layers

```text
L6  applications and adapters
    - observation
    - gateway
    - tui_dashboard

L5  common::facade
L4  common::twin_runtime
L3  common::observation_records
L0-L2 domain and FSM foundation
```

## Dependency rules

- **Allowed:** `observation -> common::facade`. The `observation` crate is an L6 adapter and may
  import live observation types (diagnostic and transition-ledger records) only through
  `common::facade` — never through `common::twin_runtime`, `common::observation_records`, or any
  other internal `common` module directly.
- **Prohibited:** `common -X-> observation`. `common` must never depend on `observation`, at
  compile time or otherwise. The dependency arrow runs one way, from L6 adapters down to `common`,
  never back up.
- **Sibling adapters:** application crates (L6) may depend on multiple L6 sibling adapters — for
  example, `tui_dashboard` may depend on `observation` in addition to `common::facade`. However, no
  L6 adapter may depend on another L6 adapter in a way that creates a cycle; the L6 layer must
  remain a directed acyclic graph rooted at `common::facade`.

## Why this document exists

`common::facade.rs` has referred to this document since it introduced the L5 boundary comment, but
the file itself did not exist until Phase 3 Task 1 created it. This document backfills that
reference with the approved layer list so the boundary comment and the crate structure agree.
