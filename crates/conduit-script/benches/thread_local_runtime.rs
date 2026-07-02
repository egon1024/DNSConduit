//! Local throughput check for thread-local Rhai engine reuse (not run by `make test`).
//!
//! Run: `make performance` or `cargo bench -p conduit-script --bench thread_local_runtime`

use conduit_config::load_yaml;
use conduit_script::testing::MockHost;
use conduit_script::{compile_from_config, run_scripts, ScriptPhase};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
    let cfg = load_yaml(yaml).expect("parse fixture");
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
    let scripting = compile_from_config(&cfg, Some(&base)).expect("compile scripts");

    let mut host = MockHost {
        id: 99,
        global_query_index: 0,
        qname: "foo.vip.example.".into(),
        ..Default::default()
    };

    let n = 10_000u32;
    let start = Instant::now();
    for _ in 0..n {
        let _ = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        host.pool = None;
    }
    let elapsed = start.elapsed();
    println!(
        "thread_local_runtime: {n} runs in {:?} ({:.0} runs/sec)",
        elapsed,
        n as f64 / elapsed.as_secs_f64()
    );
}
