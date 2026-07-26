//! Integration tests for offline `conduitctl validate`.

use std::path::PathBuf;
use std::process::Command;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config")
        .join(rel)
}

#[test]
fn validate_ok_on_minimal_fixture() {
    let bin = env!("CARGO_BIN_EXE_conduitctl");
    let config = fixture("minimal.yaml");
    let output = Command::new(bin)
        .args(["validate", "--file"])
        .arg(&config)
        .output()
        .expect("run conduitctl validate");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");
}

#[test]
fn validate_rejects_rhai_syntax_error() {
    let bin = env!("CARGO_BIN_EXE_conduitctl");
    let config = fixture("with-rhai-syntax-error.yaml");
    let output = Command::new(bin)
        .args(["validate", "--file"])
        .arg(&config)
        .output()
        .expect("run conduitctl validate");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("script"),
        "expected script compile error in stderr: {stderr}"
    );
}

#[test]
fn validate_warns_metric_collect_off_while_script_references() {
    let bin = env!("CARGO_BIN_EXE_conduitctl");
    let config = fixture("metrics-consumer-collect-removed.yaml");
    let output = Command::new(bin)
        .args(["validate", "--file"])
        .arg(&config)
        .output()
        .expect("run conduitctl validate");
    assert!(
        output.status.success(),
        "collect-off write sites must not fail validate; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("collect is off"),
        "expected collect-off warning: {stderr}"
    );
    assert!(
        stderr.contains("consumer-blat.rhai"),
        "expected script path in warning: {stderr}"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");
}
