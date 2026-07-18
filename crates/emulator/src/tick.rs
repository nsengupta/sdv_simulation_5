use common::VssSignal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickFields {
    pub rpm: u16,
    pub ambient_lux: u16,
    pub rain_detected: bool,
}

impl TickFields {
    pub fn to_signals(self) -> [VssSignal; 3] {
        [
            VssSignal::EngineRpm(self.rpm),
            VssSignal::AmbientLux(self.ambient_lux),
            VssSignal::RainDetected(self.rain_detected),
        ]
    }
}
