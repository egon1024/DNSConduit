//! Emit git revision and build profile for `conduit_build_info` at compile time.

use std::path::Path;
use std::process::Command;

fn main() {
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=CONDUIT_BUILD_PROFILE={profile}");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.join("../..");
    let git_dir = workspace_root.join(".git");

    let (revision, dirty) = if git_dir.exists() {
        register_git_rerun(&git_dir);
        git_metadata(&workspace_root)
    } else {
        ("unknown".into(), "false".into())
    };

    println!("cargo:rustc-env=CONDUIT_GIT_REVISION={revision}");
    println!("cargo:rustc-env=CONDUIT_GIT_DIRTY={dirty}");
}

fn register_git_rerun(git_dir: &Path) {
    let head = git_dir.join("HEAD");
    if head.exists() {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    let index = git_dir.join("index");
    if index.exists() {
        println!("cargo:rerun-if-changed={}", index.display());
    }
}

fn git_metadata(repo_root: &Path) -> (String, String) {
    let revision = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && revision_is_safe(s))
        .unwrap_or_else(|| "unknown".into());

    let dirty = Command::new("git")
        .current_dir(repo_root)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let dirty = if dirty { "true" } else { "false" };
    (revision, dirty.to_string())
}

/// Reject values that would break `cargo:rustc-env` or Prometheus label syntax.
fn revision_is_safe(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.bytes().all(|b| b.is_ascii_alphanumeric())
}
