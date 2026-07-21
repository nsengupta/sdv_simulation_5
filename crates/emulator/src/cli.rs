use anyhow::{Context, Result, bail};
use std::num::NonZeroUsize;
use std::time::Duration;

/// Default publish period between telemetry ticks (no demo pacing).
pub const DEFAULT_TICK_MS: u64 = 100;

const USAGE: &str = "\
usage: emulator [--readings <N>] [--tick-ms <ms>]
       [-h|--help]

Drive lifecycle and live telemetry onto vcan0 (PowerOn first, then ticks).
Omit --readings to run until Ctrl+C.
--tick-ms sets the wait between ticks (default 100). Larger values slow the
whole scenario for demos (Gateway/Dashboard stay real-time relative to CAN).

examples:
  cargo run -p emulator -- --readings 30
  cargo run -p emulator -- --readings 30 --tick-ms 400
  cargo run -p emulator
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulatorArgs {
    pub readings: Option<NonZeroUsize>,
    /// Period between telemetry ticks. Default [`DEFAULT_TICK_MS`].
    pub tick: Duration,
}

impl Default for EmulatorArgs {
    fn default() -> Self {
        Self {
            readings: None,
            tick: Duration::from_millis(DEFAULT_TICK_MS),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProbabilityOverride {
    Missing,
    Applied(f32),
    Invalid,
}

pub fn parse_args<I, S>(args: I) -> Result<EmulatorArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let values: Vec<String> = args
        .into_iter()
        .map(|value| value.as_ref().to_string())
        .collect();
    if values.iter().any(|v| v == "-h" || v == "--help") {
        bail!("{USAGE}");
    }

    let mut out = EmulatorArgs::default();
    let mut i = 0usize;
    while i < values.len() {
        match values[i].as_str() {
            "--readings" => {
                i += 1;
                let raw = values
                    .get(i)
                    .context("missing value for --readings")?;
                let parsed = raw
                    .parse::<usize>()
                    .with_context(|| format!("invalid --readings value {raw:?}"))?;
                out.readings =
                    Some(NonZeroUsize::new(parsed).context("--readings must be greater than zero")?);
            }
            "--tick-ms" => {
                i += 1;
                let raw = values
                    .get(i)
                    .context("missing value for --tick-ms")?;
                let ms = raw
                    .parse::<u64>()
                    .with_context(|| format!("invalid --tick-ms value {raw:?}"))?;
                if ms == 0 {
                    bail!("--tick-ms must be greater than zero");
                }
                out.tick = Duration::from_millis(ms);
            }
            other => bail!("unknown argument {other:?}\n{USAGE}"),
        }
        i += 1;
    }
    Ok(out)
}

pub fn parse_probability_override(raw: Option<&str>) -> Result<Option<f32>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let parsed = raw
        .trim()
        .parse::<f32>()
        .with_context(|| format!("expected a float in 0.0..=1.0, got {raw:?}"))?;
    if !(0.0..=1.0).contains(&parsed) {
        bail!("expected a float in 0.0..=1.0, got {raw:?}");
    }
    Ok(Some(parsed))
}

pub fn apply_probability_override(raw: Option<&str>, target: &mut f32) -> ProbabilityOverride {
    match parse_probability_override(raw) {
        Ok(Some(value)) => {
            *target = value;
            ProbabilityOverride::Applied(value)
        }
        Ok(None) => ProbabilityOverride::Missing,
        Err(_) => ProbabilityOverride::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_argument_is_optional_and_positive_when_present() {
        let overflowing_readings = format!("{}0", usize::MAX);

        assert_eq!(parse_args(std::iter::empty::<&str>()).unwrap().readings, None);
        assert_eq!(
            parse_args(["--readings", "30"]).unwrap().readings,
            NonZeroUsize::new(30)
        );
        assert!(parse_args(["--readings", "0"]).is_err());
        assert!(parse_args(["--readings", "abc"]).is_err());
        assert!(parse_args(["--readings", overflowing_readings.as_str()]).is_err());
        assert!(parse_args(["--readings"]).is_err());
        assert!(parse_args(["--unknown"]).is_err());
    }

    #[test]
    fn tick_ms_defaults_to_100_and_accepts_positive() {
        let def = parse_args(std::iter::empty::<&str>()).unwrap();
        assert_eq!(def.tick, Duration::from_millis(DEFAULT_TICK_MS));

        let paced = parse_args(["--tick-ms", "400"]).unwrap();
        assert_eq!(paced.tick, Duration::from_millis(400));
        assert_eq!(paced.readings, None);

        let both = parse_args(["--readings", "30", "--tick-ms", "250"]).unwrap();
        assert_eq!(both.readings, NonZeroUsize::new(30));
        assert_eq!(both.tick, Duration::from_millis(250));

        let reversed = parse_args(["--tick-ms", "250", "--readings", "10"]).unwrap();
        assert_eq!(reversed.readings, NonZeroUsize::new(10));
        assert_eq!(reversed.tick, Duration::from_millis(250));

        assert!(parse_args(["--tick-ms", "0"]).is_err());
        assert!(parse_args(["--tick-ms"]).is_err());
        assert!(parse_args(["--tick-ms", "abc"]).is_err());
    }

    #[test]
    fn help_flag_prints_usage_and_examples() {
        let err = parse_args(["--help"]).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--readings"));
        assert!(text.contains("--tick-ms"));
        assert!(text.contains("cargo run -p emulator"));
        assert!(parse_args(["-h"]).is_err());
    }

    #[test]
    fn probability_override_accepts_only_closed_unit_interval() {
        assert_eq!(parse_probability_override(None).unwrap(), None);
        assert_eq!(parse_probability_override(Some("0")).unwrap(), Some(0.0));
        assert_eq!(parse_probability_override(Some("1")).unwrap(), Some(1.0));
        assert!(parse_probability_override(Some("-0.1")).is_err());
        assert!(parse_probability_override(Some("1.1")).is_err());
        assert!(parse_probability_override(Some("not-a-number")).is_err());
    }

    #[test]
    fn valid_probability_override_mutates_target() {
        let mut target = 0.01;

        let result = apply_probability_override(Some("0.25"), &mut target);

        assert_eq!(result, ProbabilityOverride::Applied(0.25));
        assert_eq!(target, 0.25);
    }

    #[test]
    fn missing_probability_override_preserves_target() {
        let mut target = 0.01;

        let result = apply_probability_override(None, &mut target);

        assert_eq!(result, ProbabilityOverride::Missing);
        assert_eq!(target, 0.01);
    }

    #[test]
    fn invalid_probability_override_preserves_target() {
        let mut target = 0.01;

        let result = apply_probability_override(Some("invalid"), &mut target);

        assert_eq!(result, ProbabilityOverride::Invalid);
        assert_eq!(target, 0.01);
    }
}
