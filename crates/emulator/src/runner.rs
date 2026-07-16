use crate::car_physics::PhysicalCar;
use crate::sink::FrameSink;
use anyhow::Result;
use common::{LifecycleCommand, VssSignal};
use std::num::NonZeroUsize;
use std::time::Duration;

pub const TICK: Duration = Duration::from_millis(100);

pub fn run_finite<S, Sleep>(
    sink: &mut S,
    car: &mut PhysicalCar,
    readings: NonZeroUsize,
    mut sleep: Sleep,
) -> Result<()>
where
    S: FrameSink,
    Sleep: FnMut(Duration),
{
    sink.write_frame(LifecycleCommand::PowerOn.to_can_frame()?)?;

    for index in 0..readings.get() {
        for signal in car.update_and_read() {
            sink.write_frame(signal.to_can_frame()?)?;
        }
        if index + 1 < readings.get() {
            sleep(TICK);
        }
    }

    sink.write_frame(VssSignal::EngineRpm(0).to_can_frame()?)?;
    sink.write_frame(LifecycleCommand::PowerOff.to_can_frame()?)?;
    Ok(())
}
