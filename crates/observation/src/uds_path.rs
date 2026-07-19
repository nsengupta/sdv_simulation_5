//! Resolve live observation UDS paths under `<cwd>/tmp` (never system `/tmp`).

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::ObservationError;

pub const DEFAULT_UDS_FILE_NAME: &str = "observation.sock";

/// Resolve a user-supplied UDS path (or the default) under `<cwd>/tmp`.
///
/// - `None` → `<cwd>/tmp/observation.sock`
/// - bare filename (e.g. `observation.sock`) → `<cwd>/tmp/<filename>`
/// - any other path must normalize to a location inside `<cwd>/tmp`
pub fn resolve_uds_path(user: Option<&Path>) -> Result<PathBuf, ObservationError> {
    let cwd = env::current_dir().map_err(|source| ObservationError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    let tmp_dir = normalize_path(&cwd.join("tmp"));

    let candidate = match user {
        None => tmp_dir.join(DEFAULT_UDS_FILE_NAME),
        Some(path) if is_bare_filename(path) => tmp_dir.join(path),
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
    };
    let normalized = normalize_path(&candidate);

    if !normalized.starts_with(&tmp_dir) {
        return Err(ObservationError::InvalidUdsPath {
            path: normalized,
            reason: format!("must be under {}", tmp_dir.display()),
        });
    }
    if normalized == tmp_dir {
        return Err(ObservationError::InvalidUdsPath {
            path: normalized,
            reason: "path must include a socket file name".into(),
        });
    }

    Ok(normalized)
}

/// Create the parent directory of `path` (typically `<cwd>/tmp`) if missing.
pub fn ensure_tmp_parent(path: &Path) -> Result<(), ObservationError> {
    let Some(parent) = path.parent() else {
        return Err(ObservationError::InvalidUdsPath {
            path: path.to_path_buf(),
            reason: "path has no parent directory".into(),
        });
    };
    fs::create_dir_all(parent).map_err(|source| ObservationError::Io {
        path: parent.to_path_buf(),
        source,
    })
}

fn is_bare_filename(path: &Path) -> bool {
    let mut components = path.components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn default_path_is_under_cwd_tmp() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        fs::create_dir_all(dir.path().join("tmp")).unwrap();
        let path = resolve_uds_path(None).unwrap();
        assert_eq!(path, dir.path().join("tmp").join("observation.sock"));
    }

    #[test]
    fn bare_filename_resolves_under_cwd_tmp() {
        let dir = tempdir().unwrap();
        let _guard = CwdGuard::enter(dir.path());
        let path = resolve_uds_path(Some(Path::new("custom.sock"))).unwrap();
        assert_eq!(path, dir.path().join("tmp").join("custom.sock"));
    }

    #[test]
    fn rejects_system_tmp() {
        let err = resolve_uds_path(Some(Path::new("/tmp/observation.sock"))).unwrap_err();
        assert!(matches!(err, ObservationError::InvalidUdsPath { .. }));
    }

    #[test]
    fn ensure_tmp_parent_creates_directory() {
        let dir = tempdir().unwrap();
        let sock = dir.path().join("tmp").join("observation.sock");
        ensure_tmp_parent(&sock).unwrap();
        assert!(dir.path().join("tmp").is_dir());
    }
}
