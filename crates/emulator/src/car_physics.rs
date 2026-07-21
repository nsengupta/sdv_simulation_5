use common::domain_types::RPM_IDLE;
use common::vehicle_physics::calculate_speed_from_rpm;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{AmbientRoadLightModel, PhysicalWorldModelConfig, RainModel, RpmModel};

pub struct PhysicalCar {
    rpm: u16,
    ambient_lux: u16,
    rain_detected: bool,
    rpm_model: RpmModel,
    ambient_road_light_model: AmbientRoadLightModel,
    rain_model: RainModel,
}

impl PhysicalCar {
    pub fn new() -> Self {
        Self::new_with_config(PhysicalWorldModelConfig::daytime_tunnel_profile())
    }

    pub fn new_with_config(cfg: PhysicalWorldModelConfig) -> Self {
        let rpm_model = RpmModel::new(cfg.rpm);
        let ambient_road_light_model = AmbientRoadLightModel::new(cfg.ambient_road_light);
        let rain_model = RainModel::new(cfg.rain);

        Self {
            rpm: RPM_IDLE,
            ambient_lux: 850,
            rain_detected: false,
            rpm_model,
            ambient_road_light_model,
            rain_model,
        }
    }

    pub fn rpm(&self) -> u16 {
        self.rpm
    }

 /// Kinematic ground speed derived from composite wheel RPM (for debug only; not published on CAN).
    pub fn derived_speed_kph(&self) -> f64 {
        calculate_speed_from_rpm(self.rpm)
    }

    pub fn ambient_lux(&self) -> u16 {
        self.ambient_lux
    }

    pub fn rain_detected(&self) -> bool {
        self.rain_detected
    }

    pub fn update(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.rpm = self.rpm_model.next_rpm(self.rpm, now);
        self.ambient_lux = self.ambient_road_light_model.next_ambient_lux(now);
        self.rain_detected = self.rain_model.next_rain_detected();

        let target_rpm = self.rpm_model.target_rpm_for_epoch(now);
        println!(
            "DEBUG: Time={}s | CompositeRPM={} (Target={}) | DerivedSpeedKph={:.2} | AmbientLux={} | Rain={}",
            now % 60,
            self.rpm,
            target_rpm,
            self.derived_speed_kph(),
            self.ambient_lux,
            self.rain_detected
        );
    }

    pub fn update_and_read(&mut self) -> [common::VssSignal; 3] {
        self.update();
        [
            common::VssSignal::EngineRpm(self.rpm()),
            common::VssSignal::AmbientLux(self.ambient_lux()),
            common::VssSignal::RainDetected(self.rain_detected()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::PhysicalCar;
    use crate::models::{
        DAYTIME_TUNNEL_HIGH_TARGET_RPM, DAYTIME_TUNNEL_LOW_TARGET_RPM, DAYTIME_TUNNEL_RPM_CEILING,
        PhysicalWorldModelConfig, RpmModel,
    };
    use common::domain_types::RPM_IDLE;
    use common::vehicle_physics::{SPEED_EXTREME_OPERATION_THRESHOLD_KPH, calculate_speed_from_rpm};

    #[test]
    fn smoke_new_car_starts_at_idle_rpm() {
        let car = PhysicalCar::new();
        assert_eq!(car.rpm(), RPM_IDLE);
        assert!(
            (car.derived_speed_kph() - calculate_speed_from_rpm(RPM_IDLE)).abs() < f64::EPSILON
        );
        assert!((0..=1200).contains(&car.ambient_lux()));
    }

    #[test]
    fn smoke_update_keeps_values_within_expected_bounds() {
        let mut car = PhysicalCar::new();
        for _ in 0..32 {
            car.update();
            assert_eq!(car.derived_speed_kph(), calculate_speed_from_rpm(car.rpm()));
            assert!((RPM_IDLE..=DAYTIME_TUNNEL_RPM_CEILING).contains(&car.rpm()));
            assert!(car.derived_speed_kph() <= 180.0);
            assert!((0..=1200).contains(&car.ambient_lux()));
        }
    }

    #[test]
    fn high_target_derived_speed_peaks_above_threshold_within_180() {
        let speed = calculate_speed_from_rpm(DAYTIME_TUNNEL_HIGH_TARGET_RPM as u16);
        assert!(
            speed > f64::from(SPEED_EXTREME_OPERATION_THRESHOLD_KPH),
            "high target must exceed 160 km/h for ExtremeOperationWarning, got {speed}"
        );
        assert!(
            speed <= 180.0,
            "high target must stay within peak band, got {speed}"
        );
    }

    #[test]
    fn low_target_derived_speed_stays_at_or_under_threshold() {
        let speed = calculate_speed_from_rpm(DAYTIME_TUNNEL_LOW_TARGET_RPM as u16);
        assert!(
            speed <= f64::from(SPEED_EXTREME_OPERATION_THRESHOLD_KPH),
            "low target must cruise at or under 160 km/h, got {speed}"
        );
    }

    #[test]
    fn update_never_exceeds_profile_rpm_ceiling_or_180_kph() {
        let mut car = PhysicalCar::new();
        for _ in 0..200 {
            car.update();
            assert!(car.rpm() <= DAYTIME_TUNNEL_RPM_CEILING);
            assert!(car.derived_speed_kph() <= 180.0);
        }
    }

    #[test]
    fn rpm_model_high_epoch_targets_peak_band() {
        let cfg = PhysicalWorldModelConfig::daytime_tunnel_profile().rpm;
        let model = RpmModel::new(cfg.clone());
 // Even epoch bucket → high target (flip period 15).
        let target = model.target_rpm_for_epoch(0);
        assert!((target - DAYTIME_TUNNEL_HIGH_TARGET_RPM).abs() < f32::EPSILON);
        let speed = calculate_speed_from_rpm(target as u16);
        assert!(speed > f64::from(SPEED_EXTREME_OPERATION_THRESHOLD_KPH));
        assert!(speed <= 180.0);

        let low = model.target_rpm_for_epoch(cfg.target_flip_period_secs);
        assert!((low - DAYTIME_TUNNEL_LOW_TARGET_RPM).abs() < f32::EPSILON);
        assert!(
            calculate_speed_from_rpm(low as u16)
                <= f64::from(SPEED_EXTREME_OPERATION_THRESHOLD_KPH)
        );
    }
}
