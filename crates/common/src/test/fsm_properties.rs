//! Property-based tests for the FSM (`proptest` feature).
//!
//! These invariants must catch policy holes that unit demos miss — especially
//! ExtremeOperationWarning recovery on abrupt standstill (emulator trailer).

use proptest::prelude::*;
use std::time::{Duration, Instant};

use crate::fsm::{
    DomainAction, FsmEvent, FsmState, HeadlampState, TransitionNote, transition,
};
use crate::twin_runtime::twin_turn;
use crate::vehicle_physics::{LUX_OFF_THRESHOLD, LUX_ON_THRESHOLD};
use crate::vehicle_state::VehicleContext;

fn ctx_with_rpm(rpm: u16) -> VehicleContext {
    let mut ctx = VehicleContext::default();
    ctx.powertrain.wheel_rpm.front_left = rpm;
    ctx.powertrain.wheel_rpm.front_right = rpm;
    ctx.powertrain.wheel_rpm.rear_left = rpm;
    ctx.powertrain.wheel_rpm.rear_right = rpm;
    ctx.powertrain.refresh_speed();
    ctx
}

prop_compose! {
    fn arb_context()(rpm in 0..8000u16) -> VehicleContext {
        let mut ctx = VehicleContext::default();
        ctx.powertrain.wheel_rpm.front_left = rpm;
        ctx.powertrain.wheel_rpm.front_right = rpm;
        ctx.powertrain.wheel_rpm.rear_left = rpm;
        ctx.powertrain.wheel_rpm.rear_right = rpm;
        ctx.headlamp.state = HeadlampState::Off;
        ctx.powertrain.refresh_speed();
        ctx
    }
}

fn arb_fsm_event() -> impl Strategy<Value = FsmEvent> {
    // Avoid `Just(FsmEvent::...)` so this stays valid if `FsmEvent` ever drops `Clone` again.
    (0u8..6u8, any::<u16>()).prop_map(|(tag, rpm)| match tag {
        0 => FsmEvent::PowerOn,
        1 => FsmEvent::PowerOff,
        2 => FsmEvent::TimerTick,
        3 => FsmEvent::UpdateRpm(rpm),
        4 => FsmEvent::FrontHeadlampOffAck,
        _ => FsmEvent::UpdateAmbientLux(100),
    })
}

fn arb_fsm_state() -> impl Strategy<Value = FsmState> {
    prop_oneof![
        Just(FsmState::Off),
        Just(FsmState::Idle),
        Just(FsmState::Driving),
        Just(FsmState::DrivingDangerously),
        any::<u64>().prop_map(|n| {
            FsmState::ExtremeOperationWarning(
                Instant::now() - Duration::from_nanos(1 + (n % 1_000_000_000)),
            )
        }),
    ]
}

proptest! {
    #[test]
    fn test_deadband_never_emits_light_requests_when_off(
        lux in (LUX_ON_THRESHOLD + 1)..LUX_OFF_THRESHOLD
    ) {
        let mut ctx = VehicleContext::default();
        ctx.headlamp.state = HeadlampState::Off;
        let result = twin_turn(&FsmState::Idle, &ctx, &FsmEvent::UpdateAmbientLux(lux), Instant::now());
        prop_assert!(!result.actions.contains(&DomainAction::RequestFrontHeadlampOn));
        prop_assert!(!result.actions.contains(&DomainAction::RequestFrontHeadlampOff));
        prop_assert_eq!(result.modified_ctx.headlamp.state, HeadlampState::Off);
    }

    #[test]
    fn test_deadband_never_emits_light_requests_when_on(
        lux in (LUX_ON_THRESHOLD + 1)..LUX_OFF_THRESHOLD
    ) {
        let mut ctx = VehicleContext::default();
        ctx.headlamp.state = HeadlampState::On;
        let result = twin_turn(&FsmState::Driving, &ctx, &FsmEvent::UpdateAmbientLux(lux), Instant::now());
        prop_assert!(!result.actions.contains(&DomainAction::RequestFrontHeadlampOn));
        prop_assert!(!result.actions.contains(&DomainAction::RequestFrontHeadlampOff));
        prop_assert_eq!(result.modified_ctx.headlamp.state, HeadlampState::On);
    }
}

proptest! {
    #[test]
    fn test_off_to_driving_invariant(
        event in arb_fsm_event(),
        ctx in arb_context()
    ) {
        let initial_state = FsmState::Off;
        let next_state = transition(&initial_state, &event, &ctx, Instant::now());

        // INVARIANT: You can NEVER go from Off directly to Driving
        prop_assert_ne!(
            next_state.next_state,
            FsmState::Driving,
            "Illegal transition detected: Off -> Driving directly via {:?}",
            event
        );
    }

    /// PowerOff is accepted from Idle (→ PreparingToStop) and from ExtremeOperationWarning
    /// only when already stationary (→ PreparingToStop). Moving operational states reject it.
    #[test]
    fn test_power_off_shutdown_gate(
        state in arb_fsm_state(),
        ctx in arb_context()
    ) {
        let before = state.clone();
        let next = transition(&state, &FsmEvent::PowerOff, &ctx, Instant::now());
        match &before {
            FsmState::Idle => {
                prop_assert!(
                    matches!(next.next_state, FsmState::PreparingToStop(_)),
                    "Idle + PowerOff must begin shutdown, got {:?}",
                    next.next_state
                );
            }
            FsmState::ExtremeOperationWarning(_) if ctx.powertrain.is_stationary() => {
                prop_assert!(
                    matches!(next.next_state, FsmState::PreparingToStop(_)),
                    "stationary ExtremeOperationWarning + PowerOff must shut down, got {:?}",
                    next.next_state
                );
                prop_assert!(next.note.is_none());
            }
            FsmState::ExtremeOperationWarning(_) => {
                prop_assert!(
                    matches!(next.next_state, FsmState::ExtremeOperationWarning(_)),
                    "moving ExtremeOperationWarning must reject PowerOff"
                );
                prop_assert_eq!(next.note, Some(TransitionNote::RejectedPowerOff));
            }
            FsmState::Driving | FsmState::DrivingDangerously => {
                prop_assert_eq!(
                    next.next_state,
                    before,
                    "PowerOff while moving must leave state unchanged"
                );
                prop_assert_eq!(next.note, Some(TransitionNote::RejectedPowerOff));
            }
            FsmState::Off => {
                prop_assert_eq!(next.next_state, FsmState::Off);
            }
            _ => {}
        }
    }
}

proptest! {
    #[test]
    fn test_shutdown_rejected_while_moving(
        state in prop_oneof![
            Just(FsmState::Driving),
            any::<u64>().prop_map(|n| {
                FsmState::ExtremeOperationWarning(Instant::now() - Duration::from_nanos(1 + (n % 1_000_000_000)))
            }),
        ],
    ) {
        let event = FsmEvent::PowerOff;
        let ctx = ctx_with_rpm(3000);
        let next = transition(&state, &event, &ctx, Instant::now());

        prop_assert_eq!(
            next.next_state,
            state,
            "PowerOff must be ignored while speed/RPM imply motion"
        );
        prop_assert_eq!(next.note, Some(TransitionNote::RejectedPowerOff));
    }
}

proptest! {
    /// The hole the suite missed: standstill must leave ExtremeOperationWarning
    /// without waiting for TimerTick cooldown (abrupt emulator trailer / hard stop).
    #[test]
    fn extreme_operation_warning_stationary_exits_without_cooldown(
        event in arb_fsm_event(),
        age_ms in 0u64..5_000u64,
    ) {
        let began = Instant::now() - Duration::from_millis(age_ms);
        let warning = FsmState::ExtremeOperationWarning(began);
        let ctx = ctx_with_rpm(0);
        prop_assert!(ctx.powertrain.is_stationary());

        let next = transition(&warning, &event, &ctx, Instant::now());

        match event {
            FsmEvent::PowerOff => {
                prop_assert!(
                    matches!(next.next_state, FsmState::PreparingToStop(_)),
                    "PowerOff + standstill from ExtremeOperationWarning → PreparingToStop, got {:?}",
                    next.next_state
                );
            }
            _ => {
                prop_assert_eq!(
                    next.next_state,
                    FsmState::Idle,
                    "standstill + {:?} must exit ExtremeOperationWarning to Idle (no cooldown)",
                    event
                );
            }
        }
    }

    /// While still rolling under/over threshold, non-TimerTick events must not clear the warning
    /// before cooldown recovery (except when stationary — covered above).
    #[test]
    fn extreme_operation_warning_stays_while_rolling_without_recovery_tick(
        rpm in 1001u16..3000u16,
        tag in 0u8..4u8,
    ) {
        let began = Instant::now();
        let warning = FsmState::ExtremeOperationWarning(began);
        let ctx = ctx_with_rpm(rpm);
        prop_assert!(!ctx.powertrain.is_stationary());

        let event = match tag {
            0 => FsmEvent::UpdateRpm(rpm),
            1 => FsmEvent::FrontHeadlampOffAck,
            2 => FsmEvent::UpdateAmbientLux(900),
            _ => FsmEvent::TimerTick, // age 0 → recovery_ready false
        };
        let next = transition(&warning, &event, &ctx, began + Duration::from_millis(10));
        prop_assert!(
            matches!(next.next_state, FsmState::ExtremeOperationWarning(_)),
            "rolling ExtremeOperationWarning must stay on {:?}, got {:?}",
            event,
            next.next_state
        );
    }
}
