use anyhow::{Context, Result, bail};
use std::num::NonZeroUsize;

const USAGE: &str = "\
usage: emulator [--readings <positive integer>]
       [-h|--help]

Drive lifecycle and live telemetry onto vcan0 (PowerOn first, then ticks).
Omit --readings to run until Ctrl+C.

examples:
  cargo run -p emulator -- --readings 30
  cargo run -p emulator
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulatorArgs {
    pub readings: Option<NonZeroUsize>,
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
    if values.is_empty() {
        return Ok(EmulatorArgs { readings: None });
    }
    if values.len() != 2 || values[0] != "--readings" {
        bail!("{USAGE}");
    }
    let parsed = values[1]
        .parse::<usize>()
        .with_context(|| format!("invalid --readings value {:?}", values[1]))?;
    let readings = NonZeroUsize::new(parsed).context("--readings must be greater than zero")?;
    Ok(EmulatorArgs {
        readings: Some(readings),
    })
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
        assert!(parse_args(["--readings", "30", "extra"]).is_err());
        assert!(parse_args(["--readings"]).is_err());
        assert!(parse_args(["--unknown"]).is_err());
    }

    #[test]
    fn help_flag_prints_usage_and_examples() {
        let err = parse_args(["--help"]).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--readings"));
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
