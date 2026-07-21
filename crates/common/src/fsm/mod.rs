//! L2 operational FSM: mode ([`FsmState`]) + [`crate::fsm::transition_map`] only mutates mode.
//!
//! **Cut** — one twin snapshot `(FsmState, VehicleContext)` at an instant; each ledger hop is
//! entry → exit. **Quiescence** — process external + [`FsmEvent::Internal`] hops before commit.

pub mod machineries;
pub mod step;
pub mod transition_map;

pub use crate::vehicle_state::HeadlampState;
pub use machineries::{
    AssemblyId, DomainAction, FrontHeadlampIncompleteCause, FrontHeadlampSwitchDirection,
    FsmAction, FsmEvent, FsmState, Operational,
};
pub use step::{RawTransitionRecord, StepResult, step};
pub use transition_map::{TransitionNote, TransitionResult, output, transition};
