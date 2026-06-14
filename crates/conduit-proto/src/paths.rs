//! Filesystem path resolution for config-relative paths.

use std::path::{Path, PathBuf};

/// Resolve a filesystem path from config: absolute paths are used as-is; relative paths
/// join against `base_dir` (the directory containing the root config file) when set.
pub fn resolve_config_path(base_dir: Option<&Path>, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(base) = base_dir {
        base.join(p)
    } else {
        p.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn absolute_path_ignores_base_dir() {
        let base = Path::new("/etc/conduit");
        assert_eq!(
            resolve_config_path(Some(base), "/var/lib/cert.pem"),
            PathBuf::from("/var/lib/cert.pem")
        );
    }

    #[test]
    fn relative_path_joins_base_dir() {
        let base = Path::new("/etc/conduit");
        assert_eq!(
            resolve_config_path(Some(base), "scripts/policy.rhai"),
            PathBuf::from("/etc/conduit/scripts/policy.rhai")
        );
    }

    #[test]
    fn relative_path_joins_base_dir_with_parent_segment() {
        let base = Path::new("/etc/conduit");
        assert_eq!(
            resolve_config_path(Some(base), "../shared/policy.rhai"),
            PathBuf::from("/etc/conduit/../shared/policy.rhai")
        );
    }

    #[test]
    fn relative_path_without_base_dir_stays_relative() {
        assert_eq!(
            resolve_config_path(None, "scripts/policy.rhai"),
            PathBuf::from("scripts/policy.rhai")
        );
    }
}
