//! Gateway command-line parsing for observation capture and exclusive live modes.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use observation::{DEFAULT_UDS_FILE_NAME, resolve_uds_path};

const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 60;

const USAGE: &str = "\
usage: gateway --uds <path> | --zenoh --keyexpr <expr> | --no-live
       [--observation-dir <dir>] [--connect-timeout <secs>]
       [--print-transitions-only] [--trace-actuation-ingress]
       [-h|--help]

Exactly one live mode is required (no default).

examples:
  cargo run -p gateway -- --uds observation.sock --connect-timeout 60
  cargo run -p gateway -- --zenoh --keyexpr sdv/twin/observation --connect-timeout 60
  cargo run -p gateway -- --no-live --observation-dir observations
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayLiveMode {
    Uds(PathBuf),
    Zenoh { keyexpr: String },
    NoLive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayArgs {
    pub live: GatewayLiveMode,
    pub observation_dir: PathBuf,
    pub connect_timeout: Duration,
    pub print_transitions_only: bool,
    pub trace_actuation_ingress: bool,
}

pub fn parse_args<I, S>(args: I) -> Result<GatewayArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let values: Vec<OsString> = args.into_iter().map(|v| v.as_ref().to_owned()).collect();
    let mut uds: Option<PathBuf> = None;
    let mut zenoh = false;
    let mut no_live = false;
    let mut keyexpr: Option<String> = None;
    let mut observation_dir = PathBuf::from("observations");
    let mut connect_timeout_secs = DEFAULT_CONNECT_TIMEOUT_SECS;
    let mut print_transitions_only = false;
    let mut trace_actuation_ingress = false;
    let mut i = 0;
    while i < values.len() {
        let arg = values[i].as_os_str();
        if arg == OsStr::new("-h") || arg == OsStr::new("--help") {
            bail!("{USAGE}");
        } else if arg == OsStr::new("--uds") {
            i += 1;
            let Some(path) = values.get(i) else {
                bail!("{USAGE}");
            };
            if path.is_empty() {
                bail!("--uds requires a non-empty path");
            }
            if uds.is_some() || zenoh || no_live {
                bail!("{USAGE}");
            }
            uds = Some(PathBuf::from(path));
        } else if arg == OsStr::new("--zenoh") {
            if uds.is_some() || zenoh || no_live {
                bail!("{USAGE}");
            }
            zenoh = true;
        } else if arg == OsStr::new("--no-live") {
            if uds.is_some() || zenoh || no_live {
                bail!("{USAGE}");
            }
            no_live = true;
        } else if arg == OsStr::new("--keyexpr") {
            i += 1;
            let Some(raw) = values.get(i) else {
                bail!("{USAGE}");
            };
            if raw.is_empty() {
                bail!("--keyexpr requires a non-empty expression");
            }
            if keyexpr.is_some() {
                bail!("{USAGE}");
            }
            keyexpr = Some(raw.to_string_lossy().into_owned());
        } else if arg == OsStr::new("--observation-dir") {
            i += 1;
            let Some(path) = values.get(i) else {
                bail!("--observation-dir requires a path");
            };
            if path.is_empty() {
                bail!("--observation-dir requires a non-empty path");
            }
            observation_dir = PathBuf::from(path);
        } else if arg == OsStr::new("--connect-timeout") {
            i += 1;
            let Some(raw) = values.get(i) else {
                bail!("--connect-timeout requires seconds");
            };
            let text = raw.to_string_lossy();
            connect_timeout_secs = text
                .parse::<u64>()
                .with_context(|| format!("invalid --connect-timeout value: {text}"))?;
        } else if arg == OsStr::new("--print-transitions-only") {
            print_transitions_only = true;
        } else if arg == OsStr::new("--trace-actuation-ingress") {
            trace_actuation_ingress = true;
        } else {
            bail!("{USAGE}");
        }
        i += 1;
    }

    // Ledger-only mode does not require a live-mode flag.
    if print_transitions_only && uds.is_none() && !zenoh && !no_live {
        return Ok(GatewayArgs {
            live: GatewayLiveMode::NoLive,
            observation_dir,
            connect_timeout: Duration::from_secs(connect_timeout_secs),
            print_transitions_only,
            trace_actuation_ingress,
        });
    }

    let live = match (uds, zenoh, no_live) {
        (Some(path), false, false) => {
            if keyexpr.is_some() {
                bail!("{USAGE}");
            }
            GatewayLiveMode::Uds(resolve_uds_path(Some(&path)).map_err(|err| anyhow::anyhow!(err))?)
        }
        (None, true, false) => {
            let Some(keyexpr) = keyexpr.filter(|k| !k.is_empty()) else {
                bail!("{USAGE}");
            };
            GatewayLiveMode::Zenoh { keyexpr }
        }
        (None, false, true) => {
            if keyexpr.is_some() {
                bail!("{USAGE}");
            }
            GatewayLiveMode::NoLive
        }
        _ => bail!("{USAGE}"),
    };

    Ok(GatewayArgs {
        live,
        observation_dir,
        connect_timeout: Duration::from_secs(connect_timeout_secs),
        print_transitions_only,
        trace_actuation_ingress,
    })
}

pub fn default_uds_path() -> Result<PathBuf> {
    resolve_uds_path(None).map_err(|err| anyhow::anyhow!(err))
}

pub fn default_uds_file_name() -> &'static str {
    DEFAULT_UDS_FILE_NAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::Path;
    use tempfile::tempdir;

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn enter(path: &Path) -> Self {
            let original = env::current_dir().unwrap();
            env::set_current_dir(path).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    #[test]
    fn requires_exactly_one_live_mode() {
        assert!(parse_args(std::iter::empty::<&str>()).is_err());
        assert!(parse_args(["--uds", "observation.sock", "--no-live"]).is_err());
        assert!(parse_args(["--zenoh"]).is_err());
        assert!(parse_args(["--zenoh", "--keyexpr", ""]).is_err());
    }

    #[test]
    fn accepts_no_live() {
        let args = parse_args(["--no-live"]).unwrap();
        assert_eq!(args.live, GatewayLiveMode::NoLive);
    }

    #[test]
    fn accepts_zenoh_with_keyexpr() {
        let args = parse_args(["--zenoh", "--keyexpr", "sdv/twin/observation"]).unwrap();
        assert_eq!(
            args.live,
            GatewayLiveMode::Zenoh {
                keyexpr: "sdv/twin/observation".into()
            }
        );
    }

    #[test]
    fn help_flag_prints_examples() {
        let err = parse_args(["--help"]).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--uds"));
        assert!(text.contains("--zenoh"));
        assert!(text.contains("--no-live"));
        assert!(text.contains("sdv/twin/observation"));
    }

    #[test]
    fn uds_bare_filename_resolves_under_cwd_tmp() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        let args = parse_args(["--uds", "observation.sock"]).unwrap();
        assert_eq!(
            args.live,
            GatewayLiveMode::Uds(dir.path().join("tmp").join("observation.sock"))
        );
    }

    #[test]
    fn rejects_system_tmp_uds() {
        let err = parse_args(["--uds", "/tmp/observation.sock"]).unwrap_err();
        assert!(err.to_string().contains("UDS path must be under"));
    }
}
