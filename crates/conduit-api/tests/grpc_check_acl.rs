mod support;

use conduit_config::load_yaml;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::CheckAclRequest;
use std::net::SocketAddr;

#[tokio::test]
async fn check_acl_returns_per_listener_results() {
    let yaml = include_str!("../../../tests/fixtures/config/with-acls.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-acls.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let endpoint = format!("http://{local_addr}");
    let mut client = ConduitControlClient::connect(endpoint)
        .await
        .expect("connect");

    let resp = client
        .check_acl(CheckAclRequest {
            ip: "10.1.2.3".into(),
            listener: None,
        })
        .await
        .expect("check_acl")
        .into_inner();

    assert_eq!(resp.ip, "10.1.2.3");
    assert_eq!(resp.results.len(), 2);

    let public = resp
        .results
        .iter()
        .find(|r| r.listener == "public")
        .expect("public");
    assert_eq!(public.decision, "admit");
    assert_eq!(public.action, "accept");
    assert_eq!(public.matched, "corp_nets");

    let internal = resp
        .results
        .iter()
        .find(|r| r.listener == "internal")
        .expect("internal");
    assert_eq!(internal.decision, "tag");
    assert_eq!(internal.tag.as_deref(), Some("corp"));
    assert_eq!(internal.matched, "corp_nets");

    let filtered = client
        .check_acl(CheckAclRequest {
            ip: "203.0.113.50".into(),
            listener: Some("public".into()),
        })
        .await
        .expect("check_acl filtered")
        .into_inner();
    assert_eq!(filtered.results.len(), 1);
    assert_eq!(filtered.results[0].decision, "drop");
    assert_eq!(filtered.results[0].matched, "block_nets");

    let err = client
        .check_acl(CheckAclRequest {
            ip: "10.1.2.3".into(),
            listener: Some("missing".into()),
        })
        .await
        .expect_err("unknown listener");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    let bad_ip = client
        .check_acl(CheckAclRequest {
            ip: "not-an-ip".into(),
            listener: None,
        })
        .await
        .expect_err("bad ip");
    assert_eq!(bad_ip.code(), tonic::Code::InvalidArgument);
}
