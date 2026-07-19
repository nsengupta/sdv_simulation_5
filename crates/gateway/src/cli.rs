//! Gateway command-line parsing for observation capture and optional live UDS.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use observation::{resolve_uds_path, DEFAULT_UDS_FILE_NAME};

const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayArgs {
    /// When set, bind/accept a live Dashboard before install (resolved under `<cwd>/tmp`).
    pub uds: Option<PathBuf>,
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
    let mut observation_dir = PathBuf::from("observations");
    let mut connect_timeout_secs = DEFAULT_CONNECT_TIMEOUT_SECS;
    let mut print_transitions_only = false;
    let mut trace_actuation_ingress = false;
    let mut i = 0;
    while i < values.len() {
        let arg = values[i].as_os_str();
        if arg == OsStr::new("--uds") {
            i += 1;
            let Some(path) = values.get(i) else {
                bail!("usage: gateway [--uds <path>] [--observation-dir <dir>] [--connect-timeout <secs>]");
            };
            if path.is_empty() {
                bail!("--uds requires a non-empty path");
            }
            uds = Some(PathBuf::from(path));
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
            connect_timeout_secs = text.parse::<u64>().with_context(|| {
                format!("invalid --connect-timeout value: {text}")
            })?;
        } else if arg == OsStr::new("--print-transitions-only") {
            print_transitions_only = true;
        } else if arg == OsStr::new("--trace-actuation-ingress") {
            trace_actuation_ingress = true;
        } else {
            bail!(
                "usage: gateway [--uds <path>] [--observation-dir <dir>] [--connect-timeout <secs>] [--print-transitions-only] [--trace-actuation-ingress]"
            );
        }
        i += 1;
    }

    let uds = match uds {
        None => None,
        Some(path) => Some(
            resolve_uds_path(Some(&path)).map_err(|err| anyhow::anyhow!(err))?,
        ),
    };

    Ok(GatewayArgs {
        uds,
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
    fn defaults_are_headless_file_capture() {
        let args = parse_args(std::iter::empty::<&str>()).unwrap();
        assert_eq!(args.uds, None);
        assert_eq!(args.observation_dir, PathBuf::from("observations"));
        assert_eq!(args.connect_timeout, Duration::from_secs(60));
        assert!(!args.print_transitions_only);
    }

    #[test]
    fn uds_bare_filename_resolves_under_cwd_tmp() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        let args = parse_args(["--uds", "observation.sock"]).unwrap();
        assert_eq!(
            args.uds.as_deref(),
            Some(dir.path().join("tmp").join("observation.sock").as_path())
        );
    }

    #[test]
    fn rejects_system_tmp_uds() {
        let err = parse_args(["--uds", "/tmp/observation.sock"]).unwrap_err();
        assert!(err.to_string().contains("UDS path must be under"));
    }
}
