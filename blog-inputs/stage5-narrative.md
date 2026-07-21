+++
title = "Prototyping a Software Defined Vehicle - Stage V"
date = 2026-07-20
draft = true

[taxonomies]
tags = ["sdv-prototype"]
+++

### Preface

[Stage IV](@/blog/Prototype-Software-Defined-Vehicle-4/index.md) fixed coordination inside the
Brain: a reorder buffer (ROB), explicit PreparingToStart/Stop, and a second assembly (Wiper).
The vehicle bus stayed **CAN**. The twin and ROB still sit in `common`.

**Stage V is about operability at the process boundary.**

We set out to run **Gateway** and **Dashboard** as separate processes from the start of this
simulation’s design work — Gateway owns the twin; Dashboard is an observation consumer — and
we refactored design and code toward that end until it matched. What we describe here is the
**final arrangement**, not a story of “we used to ship UI-owns-twin and then split.”

---

### Final arrangement

Five independently runnable binaries share the bus and observation path:

| Process | Role |
|---------|------|
| **Gateway** | Sole twin owner: install, CAN ingress, actuation, observation file tee, optional live publish |
| **Dashboard** | Observation-only TUI (driver / engineer / ledger tail); no PowerOn keys, no mailbox injection |
| **Emulator** | Finite or Ctrl+C CAN lifecycle + RPM / lux / rain |
| **Headlamp / Wiper actuators** | CMD responses on `vcan0` |

Observation is a first-class product of the Gateway:

- **Files always:** `ObservationTee` converts each twin record once and writes
  `manifest.json`, `diagnostic.jsonl`, and `ledger.jsonl` under `./observations/<run-id>/`.
- **Live is exclusive:** operators pick **exactly one** mode — `--uds PATH`, or
  `--zenoh --keyexpr EXPR`, or Gateway `--no-live` for headless capture. Live payloads share
  the same envelope shape as the files (schema currently **v3**).
- **Install gate:** Gateway waits for a Dashboard (UDS accept or first Zenoh matching
  subscriber) before installing the twin, so the live consumer is attached when the session
  starts.

Published `common` structs remain the source of truth; JSON mirrors them field-for-field.
Presentation (glyphs, colours, labels) stays **receiver-side** on the Dashboard.

On the Driver pane we show Notice, a zoned speed bar, visibility (lux band), and weather /
wiper glyphs with text. On the Engineer pane we show state, last event, and Headlamp / Wiper
context — not a duplicate Weather line, and not a placeholder Active ROB counter we cannot
honestly compute from ledger hops alone.

---

### Why we rejected “Dashboard owns Gateway”

Along the way we considered (and briefly sketched) a design where the **Dashboard process
owned or hosted the twin** — UI and Gateway in one address space, observation as an in-process
concern. We did not treat that as the lasting product shape. Had we stayed there, we would
have hit limits that conflict with how we want to operate the prototype:

1. **Quitting the UI would tear down the car.** Restarting the TUI would mean restarting the
   twin, actuators session context, and observation run — not something we want for a Gateway
   we treat as the vehicle edge.
2. **No clean second observer.** Attaching another consumer, swapping UIs, or running headless
   capture without a TUI would fight the ownership model instead of using a sink/source
   boundary.
3. **Lifecycle trust would blur.** Dashboard keys or mailbox injection would compete with CAN
   `0x100` PowerOn/PowerOff from the Emulator; we want a single trust boundary: the twin reacts
   to the bus, the UI only displays what the twin emits.
4. **Archival would stay accidental.** Diagnostics and ledger as in-memory Rust types, without
   a versioned on-disk contract, make replay and “what happened this run?” harder to keep
   honest.

So we aimed at **Gateway owns twin; Dashboard subscribes**, and we moved the code there
gradually (observation library, file tee, then live UDS, then peer Zenoh, then presentation
honesty for weather/wiper). The counterfactual above is why that target mattered — not a
description of what Stage IV shipped as our long-term topology.

---

### Design principles we keep

- **Emit facts, format at the edge.** No emoji in Twin payloads; glyphs are Dashboard-only.
- **Wide streams, selective collectors.** The twin emits rich diagnostics and ledger context;
  panes filter. We do not thin the twin to one UI’s needs.
- **Do not invent ROB depth on a ledger hop.** Active ROB turns need emit-on-queue-change; we
  deferred an honest live `N` rather than show a misleading `—` or a post-drain `0`.
- **CAN first for the vehicle bus.** Zenoh in this stage is an **observation** carrier, not a
  bus replacement.
- **README is repo truth; this file is accompanying prose.**

---

### What we deliberately left open

Shutdown / disband, standalone replay, coloured visibility swatches, Notice severity colours,
and Active ROB on the Engineer pane. We list those as known gaps in the repository README —
not as buried chat notes.

---

### Pointers into the repo

| Topic | Doc |
|-------|-----|
| What the tree is / how to run | `README.md` |
| Roadmap + TBDs | `docs/PLAN.md` |
| Stage V decisions | `docs/DESIGN.md` |
| Twin / ROB (Stage IV) | `docs/archive/DESIGN-iteration-4.md` |
| Historical design notes | `docs/archive/` |
