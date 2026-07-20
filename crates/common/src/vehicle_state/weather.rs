//! Weather zone (L1): environment snapshot for rain policy input.
//!
//! Durable `raining` is updated on `FsmEvent::RainsStarted` / `RainsStopped` in
//! `zone_turn` so ledger hops carry standing weather status.

/// L1 weather snapshot — rain detected for the current twin context.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeatherContext {
    /// `true` after rains started until rains stopped.
    pub raining: bool,
}

#[cfg(test)]
mod tests {
    use crate::fsm::{FsmEvent, FsmState};
    use crate::twin_runtime::zone_replies::ZoneReplies;
    use crate::twin_runtime::zone_turn::zone_turn;
    use crate::vehicle_state::VehicleContext;
    use std::time::Instant;

    #[test]
    fn rains_started_sets_weather_raining_true() {
        let ctx = VehicleContext::default();
        assert!(!ctx.weather.raining);
        let result = zone_turn(
            &ctx,
            &FsmEvent::RainsStarted,
            &FsmState::Idle,
            Instant::now(),
            &ZoneReplies::simulate_locally(),
        );
        assert!(result.ctx.weather.raining);
    }

    #[test]
    fn rains_stopped_clears_weather_raining() {
        let mut ctx = VehicleContext::default();
        ctx.weather.raining = true;
        let result = zone_turn(
            &ctx,
            &FsmEvent::RainsStopped,
            &FsmState::Idle,
            Instant::now(),
            &ZoneReplies::simulate_locally(),
        );
        assert!(!result.ctx.weather.raining);
    }
}
