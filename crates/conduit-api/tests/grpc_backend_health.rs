mod support;

use conduit_proto::control::backend_health_client::BackendHealthClient;
use conduit_proto::control::{
    BackendHealthFilter, GetBackendHealthRequest, HealthControlAction, HealthLiveness, HealthScope,
    HealthScopeLevel, HealthScopeState, SetHealthControlRequest,
};
use std::net::SocketAddr;

#[tokio::test]
async fn get_backend_health_lists_configured_backends() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::health_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let mut client = BackendHealthClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let response = client
        .get_backend_health(GetBackendHealthRequest { filter: None })
        .await
        .expect("get backend health")
        .into_inner();

    assert_eq!(response.entries.len(), 2);
    for entry in &response.entries {
        assert_eq!(entry.pool, "default");
        assert!(
            entry.backend == "127.0.0.1:5300" || entry.backend == "127.0.0.1:5301",
            "unexpected backend {}",
            entry.backend
        );
        // Optimistic initial_state: applied up, eligible.
        assert_eq!(entry.applied, HealthLiveness::Up as i32);
        assert!(entry.eligible);
        assert_eq!(entry.scope_state, HealthScopeState::Automatic as i32);
    }
}

#[tokio::test]
async fn get_backend_health_empty_when_health_not_configured() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let mut client = BackendHealthClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let response = client
        .get_backend_health(GetBackendHealthRequest { filter: None })
        .await
        .expect("get backend health")
        .into_inner();

    assert!(response.entries.is_empty());
}

#[tokio::test]
async fn set_health_control_drain_and_resume_via_grpc() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::health_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(
        addr,
        snapshots.clone(),
        effective,
        configurator,
        tracing,
        base_dir,
    )
    .await
    .expect("start server");

    let mut client = BackendHealthClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let backend = "127.0.0.1:5301";
    let set_down = client
        .set_health_control(SetHealthControlRequest {
            scope: Some(HealthScope {
                level: HealthScopeLevel::Backend.into(),
                pool: Some("default".into()),
                backend: Some(backend.into()),
            }),
            action: HealthControlAction::SetDown.into(),
        })
        .await
        .expect("set down")
        .into_inner();

    assert_eq!(set_down.results.len(), 1);
    let drained = &set_down.results[0];
    assert_eq!(drained.applied, HealthLiveness::Down as i32);
    assert_eq!(drained.scope_state, HealthScopeState::Frozen as i32);
    assert_eq!(drained.pool.as_deref(), Some("default"));
    assert_eq!(drained.backend.as_deref(), Some(backend));

    // Probes still see the backend up; applied stays down while frozen.
    let state = snapshots
        .health()
        .get("default", backend.parse().unwrap())
        .expect("backend state");
    state.record_success(1, 0.2, 1.0);
    assert_eq!(state.observed(), conduit_core::health::Health::Up);
    assert_eq!(state.applied(), conduit_core::health::Health::Down);

    let filtered = client
        .get_backend_health(GetBackendHealthRequest {
            filter: Some(BackendHealthFilter {
                pool: Some("default".into()),
                backend: Some(backend.into()),
            }),
        })
        .await
        .expect("get filtered")
        .into_inner();
    assert_eq!(filtered.entries.len(), 1);
    let entry = &filtered.entries[0];
    assert_eq!(entry.observed, HealthLiveness::Up as i32);
    assert_eq!(entry.applied, HealthLiveness::Down as i32);
    assert!(!entry.eligible);
    assert_eq!(entry.scope_state, HealthScopeState::Frozen as i32);

    let resume = client
        .set_health_control(SetHealthControlRequest {
            scope: Some(HealthScope {
                level: HealthScopeLevel::Backend.into(),
                pool: Some("default".into()),
                backend: Some(backend.into()),
            }),
            action: HealthControlAction::ResumeAutomatic.into(),
        })
        .await
        .expect("resume")
        .into_inner();

    assert_eq!(resume.results.len(), 1);
    assert_eq!(resume.results[0].applied, HealthLiveness::Up as i32);
    assert_eq!(
        resume.results[0].scope_state,
        HealthScopeState::Automatic as i32
    );
    assert_eq!(state.applied(), conduit_core::health::Health::Up);
}

#[tokio::test]
async fn set_health_control_fails_when_health_not_configured() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let mut client = BackendHealthClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let err = client
        .set_health_control(SetHealthControlRequest {
            scope: Some(HealthScope {
                level: HealthScopeLevel::Global.into(),
                pool: None,
                backend: None,
            }),
            action: HealthControlAction::Freeze.into(),
        })
        .await
        .expect_err("must fail without health config");

    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}
