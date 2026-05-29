//! Compile-time build metadata embedded in `conduit_build_info`.

/// Crate / workspace semver from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit (`git rev-parse --short HEAD`), or `unknown` outside a git tree.
pub const REVISION: &str = env!("CONDUIT_GIT_REVISION");

/// `true` when `git status --porcelain` is non-empty at build time; otherwise `false`.
pub const DIRTY: &str = env!("CONDUIT_GIT_DIRTY");

/// Cargo build profile (`debug` or `release`).
pub const BUILD_PROFILE: &str = env!("CONDUIT_BUILD_PROFILE");

/// Const labels for `conduit_build_info` (Prometheus + OTEL via shared scrape text).
pub fn label_pairs() -> [(&'static str, &'static str); 4] {
    [
        ("version", VERSION),
        ("revision", REVISION),
        ("dirty", DIRTY),
        ("profile", BUILD_PROFILE),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_label_is_boolean_string() {
        assert!(DIRTY == "true" || DIRTY == "false");
    }

    #[test]
    fn revision_and_profile_are_non_empty() {
        assert!(!REVISION.is_empty());
        assert!(!BUILD_PROFILE.is_empty());
    }

    #[test]
    fn version_matches_cargo_pkg() {
        assert!(!VERSION.is_empty());
    }
}
