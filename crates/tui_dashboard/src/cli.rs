//! Strict command-line parsing for the observation-only Dashboard.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use observation::resolve_uds_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardArgs {
    /// Resolved UDS path under `<cwd>/tmp`.
    pub uds: PathBuf,
}

pub fn parse_args<I, S>(args: I) -> anyhow::Result<DashboardArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let values: Vec<OsString> = args.into_iter().map(|v| v.as_ref().to_owned()).collect();
    let uds = match values.as_slice() {
        [] => resolve_uds_path(None).map_err(|err| anyhow::anyhow!(err))?,
        [flag, path]
            if flag.as_os_str() == OsStr::new("--uds") && !path.is_empty() =>
        {
            resolve_uds_path(Some(std::path::Path::new(path)))
                .map_err(|err| anyhow::anyhow!(err))?
        }
        _ => anyhow::bail!("usage: tui_dashboard [--uds <path-under-cwd/tmp>]"),
    };
    Ok(DashboardArgs { uds })
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
    fn default_uds_is_under_cwd_tmp() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        let args = parse_args(std::iter::empty::<&str>()).unwrap();
        assert_eq!(args.uds, dir.path().join("tmp").join("observation.sock"));
    }

    #[test]
    fn explicit_uds_filename_is_accepted() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        assert_eq!(
            parse_args(["--uds", "custom.sock"]).unwrap().uds,
            dir.path().join("tmp").join("custom.sock")
        );
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
    }
}
