//! Finite / Ctrl+C CAN session: PowerOn → telemetry ticks → abrupt RPM0 + PowerOff.
//!
//! Inter-tick wait comes from [`SessionConfig::tick`] (CLI `--tick-ms`, default 100 ms).

use crate::sink::FrameSink;
use crate::source::TelemetrySource;
use crate::tick::TickFields;
use anyhow::Result;
use common::{LifecycleCommand, VssSignal};
use std::num::NonZeroUsize;
use std::time::Duration;

use crate::cli::DEFAULT_TICK_MS;

/// Default tick period when callers omit [`SessionConfig::tick`] (same as CLI default).
pub const TICK: Duration = Duration::from_millis(DEFAULT_TICK_MS);
const SLEEP_SLICE: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    pub max_readings: Option<NonZeroUsize>,
    /// Sleep between telemetry ticks after each published reading (except after the last).
    pub tick: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_readings: None,
            tick: TICK,
        }
    }
}

/// End-of-session trailer: abrupt RPM→0 then PowerOff.
///
/// TODO(emulator-ramp): accelerate/decelerate gradually for live ticks, Ctrl+C, and
/// `--readings` alike. Until then the Twin FSM must accept abrupt standstill (see
/// `ExtremeOperationWarning` recovery when `speed_kph == 0`).
pub fn controlled_stop<S: FrameSink>(sink: &mut S) -> Result<()> {
    sink.write_frame(VssSignal::EngineRpm(0).to_can_frame()?)?;
    sink.write_frame(LifecycleCommand::PowerOff.to_can_frame()?)?;
    Ok(())
}

pub fn run_session<S, Src, Sleep, Stopped>(
    sink: &mut S,
    source: &mut Src,
    config: SessionConfig,
    mut sleep: Sleep,
    mut stop_requested: Stopped,
) -> Result<()>
where
    S: FrameSink,
    Src: TelemetrySource,
    Sleep: FnMut(Duration),
    Stopped: FnMut() -> bool,
{
    sink.write_frame(LifecycleCommand::PowerOn.to_can_frame()?)?;

    let mut emitted = 0usize;
    loop {
        if stop_requested() {
            break;
        }
        if let Some(max) = config.max_readings {
            if emitted >= max.get() {
                break;
            }
        }

        let Some(tick) = source.next_tick()? else {
            break;
        };
        write_tick(sink, tick)?;
        emitted += 1;

        let limit_reached = config
            .max_readings
            .is_some_and(|max| emitted >= max.get());
        if stop_requested() || limit_reached {
            break;
        }

        interruptible_sleep(&mut sleep, config.tick, &mut stop_requested);
    }

    controlled_stop(sink)
}

fn write_tick<S: FrameSink>(sink: &mut S, tick: TickFields) -> Result<()> {
    for signal in tick.to_signals() {
        sink.write_frame(signal.to_can_frame()?)?;
    }
    Ok(())
}

fn interruptible_sleep<Sleep, Stopped>(
    sleep: &mut Sleep,
    total: Duration,
    stop_requested: &mut Stopped,
) where
    Sleep: FnMut(Duration),
    Stopped: FnMut() -> bool,
{
    let mut remaining = total;
    while remaining > Duration::ZERO {
        if stop_requested() {
            return;
        }
        let slice = remaining.min(SLEEP_SLICE);
        sleep(slice);
        remaining = remaining.saturating_sub(slice);
    }
}
