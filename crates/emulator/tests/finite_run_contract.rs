use anyhow::{Result, anyhow};
use common::{LifecycleCommand, VssSignal};
use emulator::car_physics::PhysicalCar;
use emulator::runner::{TICK, run_finite};
use emulator::sink::FrameSink;
use socketcan::CanFrame;
use std::num::NonZeroUsize;

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

#[test]
fn finite_run_writes_exact_order_and_count() {
    let mut sink = RecordingSink::default();
    let mut car = PhysicalCar::new();
    let mut sleeps = Vec::new();

    run_finite(
        &mut sink,
        &mut car,
        NonZeroUsize::new(2).unwrap(),
        |duration| sleeps.push(duration),
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
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[4]),
        Some(VssSignal::EngineRpm(_))
    ));
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[5]),
        Some(VssSignal::AmbientLux(_))
    ));
    assert!(matches!(
        VssSignal::from_can_frame(&sink.frames[6]),
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
    assert_eq!(sleeps, vec![TICK]);
}

#[test]
fn sink_error_stops_the_run() {
    let mut sink = RecordingSink {
        frames: Vec::new(),
        fail_after: Some(1),
    };
    let mut car = PhysicalCar::new();

    let error = run_finite(&mut sink, &mut car, NonZeroUsize::new(1).unwrap(), |_| {}).unwrap_err();

    assert!(error.to_string().contains("injected sink failure"));
    assert_eq!(sink.frames.len(), 1);
}
