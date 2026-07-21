//! Integration tests for offline `conduitctl acl check --file`.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config")
        .join(rel)
}

#[test]
fn acl_check_file_reports_json_for_all_listeners() {
    let bin = env!("CARGO_BIN_EXE_conduitctl");
    let config = fixture("with-acls.yaml");
    let output = Command::new(bin)
        .args(["acl", "check", "10.1.2.3", "--file"])
        .arg(&config)
        .output()
        .expect("run conduitctl acl check");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["ip"], "10.1.2.3");
    assert_eq!(v["source"], "file");
    let results = v["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    let public = results
        .iter()
        .find(|r| r["listener"] == "public")
        .expect("public");
    assert_eq!(public["decision"], "admit");
    assert_eq!(public["action"], "accept");
    let internal = results
        .iter()
        .find(|r| r["listener"] == "internal")
        .expect("internal");
    assert_eq!(internal["decision"], "tag");
    assert_eq!(internal["tag"], "corp");
}

#[test]
fn acl_check_file_filters_listener_and_rejects_unknown() {
    let bin = env!("CARGO_BIN_EXE_conduitctl");
    let config = fixture("with-acls.yaml");

    let ok = Command::new(bin)
        .args([
            "acl",
            "check",
            "203.0.113.9",
            "--listener",
            "public",
            "--file",
        ])
        .arg(&config)
        .output()
        .expect("run filtered");
    assert!(
        ok.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let v: Value = serde_json::from_str(&String::from_utf8_lossy(&ok.stdout)).unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    assert_eq!(v["results"][0]["decision"], "drop");

    let bad = Command::new(bin)
        .args(["acl", "check", "10.0.0.1", "--listener", "nope", "--file"])
        .arg(&config)
        .output()
        .expect("run unknown listener");
    assert!(!bad.status.success());
}
