use anyhow::{Result, anyhow};
use common::{LifecycleCommand, VssSignal};
use emulator::runner::{SessionConfig, TICK, run_session};
use emulator::sink::FrameSink;
use emulator::source::{LivePhysicsSource, TelemetrySource};
use emulator::tick::TickFields;
use socketcan::CanFrame;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Default)]
struct RecordingSink {
    frames: Vec<CanFrame>,
    fail_after: Option<usize>,
}

impl FrameSink for RecordingSink {
    fn write_frame(&mut self, frame: CanFrame) -> Result<()> {
        if self.fail_after == Some(self.frames.len()) {
            return Err(anyhow!("injected sink failure"));
        }
        self.frames.push(frame);
        Ok(())
    }
}

struct FixedSource {
    ticks: Vec<TickFields>,
    index: usize,
}

impl TelemetrySource for FixedSource {
    fn next_tick(&mut self) -> Result<Option<TickFields>> {
        if self.index >= self.ticks.len() {
            return Ok(None);
        }
        let tick = self.ticks[self.index];
        self.index += 1;
        Ok(Some(tick))
    }
}

#[test]
fn readings_limit_writes_exact_order_and_count() {
    let mut sink = RecordingSink::default();
    let mut source = LivePhysicsSource::new(emulator::car_physics::PhysicalCar::new());
    let mut sleeps = Vec::new();

    run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: Some(NonZeroUsize::new(2).unwrap()),
            tick: TICK,
        },
        |duration| sleeps.push(duration),
        || false,
    )
    .unwrap();

    assert_eq!(sink.frames.len(), 9); // 3N + 3
    assert_eq!(
        LifecycleCommand::from_can_frame(&sink.frames[0]),
        Some(LifecycleCommand::PowerOn)
    );
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[1]),
        Some(VssSignal::EngineRpm(_))
    ));
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[2]),
        Some(VssSignal::AmbientLux(_))
    ));
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[3]),
        Some(VssSignal::RainDetected(_))
    ));
    assert_eq!(
        VssSignal::from_can_frame(&sink.frames[7]),
        Some(VssSignal::EngineRpm(0))
    );
    assert_eq!(
        LifecycleCommand::from_can_frame(&sink.frames[8]),
        Some(LifecycleCommand::PowerOff)
    );
 // interruptible_sleep slices TICK into 10 ms pieces
    assert_eq!(sleeps.len(), 10);
    assert!(sleeps.iter().all(|d| *d == Duration::from_millis(10)));
    assert_eq!(
        sleeps.iter().copied().sum::<Duration>(),
        TICK
    );
}

#[test]
fn custom_tick_ms_controls_sleep_budget() {
    let mut sink = RecordingSink::default();
    let mut source = LivePhysicsSource::new(emulator::car_physics::PhysicalCar::new());
    let mut sleeps = Vec::new();
    let tick = Duration::from_millis(250);

    run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: Some(NonZeroUsize::new(2).unwrap()),
            tick,
        },
        |duration| sleeps.push(duration),
        || false,
    )
    .unwrap();

    assert_eq!(sleeps.iter().copied().sum::<Duration>(), tick);
    assert!(sleeps.iter().all(|d| *d == Duration::from_millis(10)));
    assert_eq!(sleeps.len(), 25);
}

#[test]
fn stop_flag_after_first_tick_writes_trailer_once() {
    let mut sink = RecordingSink::default();
    let mut source = FixedSource {
        ticks: (0..8)
            .map(|i| TickFields {
                rpm: 1000 + i as u16,
                ambient_lux: 800,
                rain_detected: false,
            })
            .collect(),
        index: 0,
    };
    let stop = AtomicBool::new(false);

    run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: None,
            tick: TICK,
        },
        |_| {
 // After the runner sleeps post-tick-1, request stop.
            stop.store(true, Ordering::SeqCst);
        },
        || stop.load(Ordering::SeqCst),
    )
    .unwrap();

 // PowerOn + 1 triple + Rpm0 + PowerOff = 6 frames
    assert_eq!(sink.frames.len(), 6);
    assert_eq!(
        VssSignal::from_can_frame(&sink.frames[4]),
        Some(VssSignal::EngineRpm(0))
    );
    assert_eq!(
        LifecycleCommand::from_can_frame(&sink.frames[5]),
        Some(LifecycleCommand::PowerOff)
    );
    let power_off_count = sink
        .frames
        .iter()
        .filter(|frame| {
            LifecycleCommand::from_can_frame(frame) == Some(LifecycleCommand::PowerOff)
        })
        .count();
    assert_eq!(power_off_count, 1);
    assert_eq!(source.index, 1);
}

#[test]
fn sink_error_stops_the_session() {
    let mut sink = RecordingSink {
        frames: Vec::new(),
        fail_after: Some(1),
    };
    let mut source = LivePhysicsSource::new(emulator::car_physics::PhysicalCar::new());

    let error = run_session(
        &mut sink,
        &mut source,
        SessionConfig {
            max_readings: Some(NonZeroUsize::new(1).unwrap()),
            tick: TICK,
        },
        |_| {},
        || false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("injected sink failure"));
    assert_eq!(sink.frames.len(), 1);
}
