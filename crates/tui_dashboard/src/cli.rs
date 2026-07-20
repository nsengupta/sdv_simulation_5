//! Strict command-line parsing for the observation-only Dashboard.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use observation::resolve_uds_path;

const USAGE: &str = "\
usage: tui_dashboard --uds <path> | --zenoh --keyexpr <expr>
       [-h|--help]

Exactly one live mode is required (no default). Dashboard always observes live.

examples:
  cargo run -p tui_dashboard -- --uds observation.sock
  cargo run -p tui_dashboard -- --zenoh --keyexpr sdv/twin/observation
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardLiveMode {
    Uds(PathBuf),
    Zenoh { keyexpr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardArgs {
    pub live: DashboardLiveMode,
}

pub fn parse_args<I, S>(args: I) -> anyhow::Result<DashboardArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let values: Vec<OsString> = args.into_iter().map(|v| v.as_ref().to_owned()).collect();
    let mut uds: Option<PathBuf> = None;
    let mut zenoh = false;
    let mut keyexpr: Option<String> = None;
    let mut i = 0;
    while i < values.len() {
        let arg = values[i].as_os_str();
        if arg == OsStr::new("-h") || arg == OsStr::new("--help") {
            anyhow::bail!("{USAGE}");
        } else if arg == OsStr::new("--uds") {
            i += 1;
            let Some(path) = values.get(i) else {
                anyhow::bail!("{USAGE}");
            };
            if path.is_empty() {
                anyhow::bail!("--uds requires a non-empty path");
            }
            if uds.is_some() || zenoh {
                anyhow::bail!("{USAGE}");
            }
            uds = Some(PathBuf::from(path));
        } else if arg == OsStr::new("--zenoh") {
            if uds.is_some() || zenoh {
                anyhow::bail!("{USAGE}");
            }
            zenoh = true;
        } else if arg == OsStr::new("--keyexpr") {
            i += 1;
            let Some(raw) = values.get(i) else {
                anyhow::bail!("{USAGE}");
            };
            if raw.is_empty() {
                anyhow::bail!("--keyexpr requires a non-empty expression");
            }
            if keyexpr.is_some() {
                anyhow::bail!("{USAGE}");
            }
            keyexpr = Some(raw.to_string_lossy().into_owned());
        } else if arg == OsStr::new("--no-live") {
            anyhow::bail!("{USAGE}");
        } else {
            anyhow::bail!("{USAGE}");
        }
        i += 1;
    }

    let live = match (uds, zenoh) {
        (Some(path), false) => {
            if keyexpr.is_some() {
                anyhow::bail!("{USAGE}");
            }
            DashboardLiveMode::Uds(
                resolve_uds_path(Some(&path)).map_err(|err| anyhow::anyhow!(err))?,
            )
        }
        (None, true) => {
            let Some(keyexpr) = keyexpr.filter(|k| !k.is_empty()) else {
                anyhow::bail!("{USAGE}");
            };
            DashboardLiveMode::Zenoh { keyexpr }
        }
        _ => anyhow::bail!("{USAGE}"),
    };

    Ok(DashboardArgs { live })
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
    fn requires_live_mode_no_default() {
        assert!(parse_args(std::iter::empty::<&str>()).is_err());
    }

    #[test]
    fn rejects_no_live() {
        assert!(parse_args(["--no-live"]).is_err());
    }

    #[test]
    fn accepts_uds() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        let args = parse_args(["--uds", "observation.sock"]).unwrap();
        assert_eq!(
            args.live,
            DashboardLiveMode::Uds(dir.path().join("tmp").join("observation.sock"))
        );
    }

    #[test]
    fn accepts_zenoh() {
        let args = parse_args(["--zenoh", "--keyexpr", "sdv/twin/observation"]).unwrap();
        assert_eq!(
            args.live,
            DashboardLiveMode::Zenoh {
                keyexpr: "sdv/twin/observation".into()
            }
        );
    }

    #[test]
    fn help_includes_examples() {
        let err = parse_args(["-h"]).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--uds"));
        assert!(text.contains("--zenoh"));
        assert!(text.contains("sdv/twin/observation"));
    }

    #[test]
    fn rejects_system_tmp_uds() {
        assert!(parse_args(["--uds", "/tmp/observation.sock"]).is_err());
    }

    #[test]
    fn malformed_dashboard_arguments_are_rejected() {
        assert!(parse_args(["--uds"]).is_err());
        assert!(parse_args(["--unknown"]).is_err());
        assert!(parse_args(["--observation-dir", "observations"]).is_err());
        assert!(parse_args(["--zenoh"]).is_err());
    }
}
