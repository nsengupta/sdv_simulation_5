use common::domain_types::{RPM_EXTREME_OPERATION_THRESHOLD, RPM_IDLE};

/// Profile RPM hard clamp: Twin derives `speed ≈ rpm * 0.114`, so 1580 → ~180 km/h peak.
pub const DAYTIME_TUNNEL_RPM_CEILING: u16 = 1580;

/// High RPM target for daytime-tunnel: ~1550 × 0.114 ≈ 177 km/h (above 160 for ExtremeOperationWarning).
pub const DAYTIME_TUNNEL_HIGH_TARGET_RPM: f32 = 1550.0;

/// Low RPM target: ~1200 × 0.114 ≈ 137 km/h (cruise under the 160 km/h threshold).
pub const DAYTIME_TUNNEL_LOW_TARGET_RPM: f32 = 1200.0;

#[derive(Debug, Clone)]
pub struct SpeedModelConfig {
    pub min_kph: f64,
    pub max_kph: f64,
    pub random_nudge_min: f64,
    pub random_nudge_max: f64,
}

#[derive(Debug, Clone)]
pub struct RpmModelConfig {
    pub idle_rpm: u16,
    pub extreme_operation_rpm: u16,
    pub redline_rpm: u16,
    pub high_target_rpm: f32,
    pub low_target_rpm: f32,
    pub target_flip_period_secs: u64,
    pub proportional_gain: f32,
    pub jitter_amplitude: f32,
}

#[derive(Debug, Clone)]
pub struct AmbientRoadLightModelConfig {
    pub min_lux: u16,
    pub max_lux: u16,
    pub baseline_daylight_lux: u16,
    pub jitter_amplitude_lux: i16,
    pub tunnel_event_probability_per_tick: f32,
    pub tunnel_lux_drop: u16,
    pub tunnel_duration_ticks_min: u16,
    pub tunnel_duration_ticks_max: u16,
    pub cycle_secs: u64,
}

#[derive(Debug, Clone)]
pub struct RainModelConfig {
 /// Per 100 ms tick, probability of rain starting when currently dry.
    pub rain_event_probability_per_tick: f32,
    pub rain_duration_ticks_min: u16,
    pub rain_duration_ticks_max: u16,
}

#[derive(Debug, Clone)]
pub struct PhysicalWorldModelConfig {
    pub speed: SpeedModelConfig,
    pub rpm: RpmModelConfig,
    pub ambient_road_light: AmbientRoadLightModelConfig,
    pub rain: RainModelConfig,
}

impl PhysicalWorldModelConfig {
 /// Demo profile: Twin derives speed from EngineRpm (`kph ≈ rpm * 0.114`).
 ///
 /// RPM is banded so derived speed usually sits under the Twin speed threshold (160 km/h)
 /// on the low target and briefly peaks ~165–180 km/h on the high target — enough for
 /// ExtremeOperationWarning demos without runaway speeds. Lighting-driven
 /// DrivingDangerously is unchanged.
    pub fn daytime_tunnel_profile() -> Self {
        Self {
            speed: SpeedModelConfig {
                min_kph: 0.0,
 // Align with kinematic peak from [`DAYTIME_TUNNEL_RPM_CEILING`] (not yet on CAN).
                max_kph: 180.0,
                random_nudge_min: -0.5,
                random_nudge_max: 0.6,
            },
            rpm: RpmModelConfig {
                idle_rpm: RPM_IDLE,
                extreme_operation_rpm: RPM_EXTREME_OPERATION_THRESHOLD,
                redline_rpm: DAYTIME_TUNNEL_RPM_CEILING,
                high_target_rpm: DAYTIME_TUNNEL_HIGH_TARGET_RPM,
                low_target_rpm: DAYTIME_TUNNEL_LOW_TARGET_RPM,
                target_flip_period_secs: 15,
                proportional_gain: 0.1,
                jitter_amplitude: 5.0,
            },
            ambient_road_light: AmbientRoadLightModelConfig {
                min_lux: 0,
                max_lux: 1200,
                baseline_daylight_lux: 850,
 // ±35 → ~815–885 lux; crosses LUX_ON (840) / LUX_OFF (860) for headlamp demo cycles.
                jitter_amplitude_lux: 35,
 // Default ≈ a tunnel every ~10 s (demo-friendly); override at startup with
 // `EMULATOR_TUNNEL_PROB` (e.g. 0.002 for infrequent tunnels). See `main.rs`.
                tunnel_event_probability_per_tick: 0.01,
                tunnel_lux_drop: 900,
                tunnel_duration_ticks_min: 20,
                tunnel_duration_ticks_max: 80,
 // TODO(profile-injection): accept full handcrafted profile selection at startup
 // (test/demo/realistic). Today only `tunnel_event_probability_per_tick` is
 // env-overridable (`EMULATOR_TUNNEL_PROB`); the rest of the profile is fixed.
                cycle_secs: 90,
            },
            rain: RainModelConfig {
 // Default ≈ rain every ~12 s when dry (demo-friendly); override at startup with
 // `EMULATOR_RAIN_PROB` (e.g. 0.002 for infrequent rain). See `main.rs`.
                rain_event_probability_per_tick: 0.008,
                rain_duration_ticks_min: 30,
                rain_duration_ticks_max: 60,
            },
        }
    }
}
