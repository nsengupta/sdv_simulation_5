use crate::car_physics::PhysicalCar;
use crate::models::PhysicalWorldModelConfig;
use crate::tick::TickFields;
use anyhow::Result;
use common::VssSignal;

pub trait TelemetrySource {
    fn next_tick(&mut self) -> Result<Option<TickFields>>;
}

pub struct LivePhysicsSource {
    car: PhysicalCar,
}

impl LivePhysicsSource {
    pub fn new(car: PhysicalCar) -> Self {
        Self { car }
    }

    pub fn from_config(cfg: PhysicalWorldModelConfig) -> Self {
        Self::new(PhysicalCar::new_with_config(cfg))
    }
}

impl TelemetrySource for LivePhysicsSource {
    fn next_tick(&mut self) -> Result<Option<TickFields>> {
        let [rpm, lux, rain] = self.car.update_and_read();
        let (VssSignal::EngineRpm(rpm), VssSignal::AmbientLux(lux), VssSignal::RainDetected(rain)) =
            (rpm, lux, rain)
        else {
            unreachable!("PhysicalCar::update_and_read always returns rpm/lux/rain");
        };
        Ok(Some(TickFields {
            rpm,
            ambient_lux: lux,
            rain_detected: rain,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PhysicalWorldModelConfig;

    #[test]
    fn live_source_emits_usual_fields_each_tick() {
        let mut source =
            LivePhysicsSource::from_config(PhysicalWorldModelConfig::daytime_tunnel_profile());
        let tick = source
            .next_tick()
            .unwrap()
            .expect("live source is open-ended");
        let signals = tick.to_signals();
        assert!(matches!(signals[0], common::VssSignal::EngineRpm(_)));
        assert!(matches!(signals[1], common::VssSignal::AmbientLux(_)));
        assert!(matches!(signals[2], common::VssSignal::RainDetected(_)));
    }

    #[test]
    fn tick_fields_signal_order_is_rpm_lux_rain() {
        let tick = TickFields {
            rpm: 1500,
            ambient_lux: 800,
            rain_detected: true,
        };
        assert_eq!(
            tick.to_signals(),
            [
                common::VssSignal::EngineRpm(1500),
                common::VssSignal::AmbientLux(800),
                common::VssSignal::RainDetected(true),
            ]
        );
    }
}
