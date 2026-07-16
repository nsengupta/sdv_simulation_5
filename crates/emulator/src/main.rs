use anyhow::Result;
use emulator::car_physics::PhysicalCar;
use emulator::cli::{ProbabilityOverride, apply_probability_override, parse_args};
use emulator::models::PhysicalWorldModelConfig;
use emulator::runner::run_finite;
use emulator::sink::SocketCanSink;
use std::{env, thread};

/// Override for the per-tick probability of *entering* a tunnel (low lux → headlamp ON).
///
/// A "tick" is one 100 ms publish loop, so probability `p` ≈ one tunnel every `1/(p·10)` seconds
/// while not already in one. Must be a float in `0.0..=1.0`; unset → the profile default `0.01`
/// (≈ a tunnel every ~10 s — frequent, good for demos). For **infrequent** tunnels try
/// `EMULATOR_TUNNEL_PROB=0.002` (≈ every ~50 s) or `0.001` (≈ every ~100 s).
const ENV_TUNNEL_PROB: &str = "EMULATOR_TUNNEL_PROB";

/// Override for the per-tick probability of *entering* rain when dry.
///
/// A "tick" is one 100 ms publish loop, so probability `p` ≈ one rain event every `1/(p·10)`
/// seconds while not already raining. Must be a float in `0.0..=1.0`; unset → the profile
/// default `0.008` (≈ rain every ~12 s). For **infrequent** rain try `EMULATOR_RAIN_PROB=0.002`
/// (≈ every ~50 s) or `0.0` to disable rain entirely.
const ENV_RAIN_PROB: &str = "EMULATOR_RAIN_PROB";

fn main() -> Result<()> {
    let args = parse_args(env::args().skip(1))?;
    let mut cfg = PhysicalWorldModelConfig::daytime_tunnel_profile();

    apply_env_probability_override(
        ENV_TUNNEL_PROB,
        &mut cfg.ambient_road_light.tunnel_event_probability_per_tick,
        "tunnel entry probability per 100 ms tick",
    );
    apply_env_probability_override(
        ENV_RAIN_PROB,
        &mut cfg.rain.rain_event_probability_per_tick,
        "rain entry probability per 100 ms tick",
    );

    let mut sink = SocketCanSink::open("vcan0")?;
    let mut car = PhysicalCar::new_with_config(cfg);
    run_finite(&mut sink, &mut car, args.readings, thread::sleep)
}

fn apply_env_probability_override(name: &str, target: &mut f32, success_text: &str) {
    let raw = env::var(name).ok();
    match apply_probability_override(raw.as_deref(), target) {
        ProbabilityOverride::Applied(value) => {
            println!("[emulator] {name}={value} — {success_text}");
        }
        ProbabilityOverride::Missing => {}
        ProbabilityOverride::Invalid => {
            if let Some(raw) = raw {
                eprintln!("[emulator] ignoring {name}={raw:?} — expected a float in 0.0..=1.0");
            }
        }
    }
}
