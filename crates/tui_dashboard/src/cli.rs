//! Strict command-line parsing for the transitional Dashboard.
//!
//! Capture is always enabled with default parent observations; the only knob is an optional
//! `--observation-dir <parent-directory>` override. Anything else is a usage error.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardArgs {
    pub observation_dir: PathBuf,
}

pub fn parse_args<I, S>(args: I) -> anyhow::Result<DashboardArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let values: Vec<OsString> = args.into_iter().map(|v| v.as_ref().to_owned()).collect();
    match values.as_slice() {
        [] => Ok(DashboardArgs {
            observation_dir: "observations".into(),
        }),
        [flag, path] if flag.as_os_str() == OsStr::new("--observation-dir") && !path.is_empty() => {
            Ok(DashboardArgs {
                observation_dir: PathBuf::from(path),
            })
        }
        _ => anyhow::bail!("usage: tui_dashboard [--observation-dir <parent-directory>]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn observation_directory_defaults_to_project_relative_observations() {
        assert_eq!(
            parse_args(std::iter::empty::<&str>()).unwrap(),
            DashboardArgs {
                observation_dir: PathBuf::from("observations")
            }
        );
    }

    #[test]
    fn explicit_observation_directory_is_accepted() {
        assert_eq!(
            parse_args(["--observation-dir", "/tmp/runs"])
                .unwrap()
                .observation_dir,
            PathBuf::from("/tmp/runs")
        );
    }

    #[test]
    fn malformed_dashboard_arguments_are_rejected() {
        assert!(parse_args(["--observation-dir"]).is_err());
        assert!(parse_args(["--unknown"]).is_err());
        assert!(parse_args(["--observation-dir", "a", "--observation-dir", "b"]).is_err());
    }
}
