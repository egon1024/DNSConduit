mod support;

use conduit_config::load_yaml;
use conduit_proto::config::{
    CacheInstance, CacheMemoryConfig, DataSource, EventSinkFilters, MetricsCategories,
    MetricsCollectEmit, MetricsConfig, MetricsGranularity,
};
use conduit_proto::control::conduit_caches_client::ConduitCachesClient;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::conduit_data_sources_client::ConduitDataSourcesClient;
use conduit_proto::control::conduit_events_client::ConduitEventsClient;
use conduit_proto::control::conduit_metrics_client::ConduitMetricsClient;
use conduit_proto::control::conduit_orchestrator_client::ConduitOrchestratorClient;
use conduit_proto::control::conduit_rhai_client::ConduitRhaiClient;
use conduit_proto::control::{
    DataSource as ControlDataSource, EventSinkFilters as ControlEventSinkFilters,
    ExportConfigRequest, GetMetricsRequest, GetOrchestratorRequest, GetRhaiRequest,
    ListCachesRequest, ListDataSourcesRequest, MetricsConfig as ControlMetricsConfig,
    PatchMetricsRequest, SetCacheMaxEntriesRequest, SetEventSinkFiltersRequest,
    SetOrchestratorLimitsRequest, SetRhaiLimitsRequest, UpsertDataSourceRequest,
};
use prost::Message;
use std::net::SocketAddr;

fn runtime_metrics_to_control(metrics: MetricsConfig) -> ControlMetricsConfig {
    let bytes = metrics.encode_to_vec();
    ControlMetricsConfig::decode(bytes.as_slice()).expect("compatible")
}

#[tokio::test]
async fn set_orchestrator_limits_changes_max_attempts() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );
    let gen0 = snapshots.generation();

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

    let mut orchestrator = ConduitOrchestratorClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect orchestrator");
    let mut control = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect control");

    // Get initial value
    let initial = orchestrator
        .get_orchestrator(GetOrchestratorRequest {})
        .await
        .expect("get")
        .into_inner();
    let initial_max = initial
        .orchestrator
        .as_ref()
        .map(|o| o.max_attempts)
        .unwrap_or(0);

    // Set new value
    let set = orchestrator
        .set_orchestrator_limits(SetOrchestratorLimitsRequest {
            max_attempts: Some(42),
            max_txn_duration_ms: None,
        })
        .await
        .expect("set-limits")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);
    assert!(set.generation > gen0);

    // Verify via Get
    let updated = orchestrator
        .get_orchestrator(GetOrchestratorRequest {})
        .await
        .expect("get")
        .into_inner();
    assert_eq!(
        updated.orchestrator.as_ref().map(|o| o.max_attempts),
        Some(42)
    );

    // Verify via Export
    let export = control
        .export_config(ExportConfigRequest {
            format: "yaml".into(),
        })
        .await
        .expect("export")
        .into_inner()
        .body;
    assert!(
        export.contains("max_attempts: 42") || export.contains("max_attempts:42"),
        "export should show updated max_attempts (was {initial_max}):\n{export}"
    );
}

#[tokio::test]
async fn set_rhai_limits_changes_max_operations() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );
    let gen0 = snapshots.generation();

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

    let mut rhai = ConduitRhaiClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect rhai");

    // Set new value
    let set = rhai
        .set_rhai_limits(SetRhaiLimitsRequest {
            max_operations: Some(123456),
            max_call_depth: None,
            hook_timeout_ms: None,
        })
        .await
        .expect("set-limits")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);
    assert!(set.generation > gen0);

    // Verify via Get
    let updated = rhai
        .get_rhai(GetRhaiRequest {})
        .await
        .expect("get")
        .into_inner();
    assert_eq!(
        updated.rhai.as_ref().map(|r| r.max_operations),
        Some(123456)
    );
}

#[tokio::test]
async fn set_cache_max_entries_on_memory_cache() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    // Add a memory cache
    file_cfg.caches.push(CacheInstance {
        name: "answers".into(),
        r#type: "memory".into(),
        max_entries: Some(1000),
        ..Default::default()
    });
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );
    let gen0 = snapshots.generation();

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

    let mut caches = ConduitCachesClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect caches");

    // List caches
    let listed = caches
        .list_caches(ListCachesRequest {})
        .await
        .expect("list")
        .into_inner();
    assert_eq!(listed.caches.len(), 1);
    assert_eq!(listed.caches[0].name, "answers");
    assert_eq!(listed.caches[0].max_entries, Some(1000));

    // Set new max_entries
    let set = caches
        .set_cache_max_entries(SetCacheMaxEntriesRequest {
            name: "answers".into(),
            max_entries: 5000,
        })
        .await
        .expect("set-max-entries")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);
    assert!(set.generation > gen0);

    // Verify via List
    let updated = caches
        .list_caches(ListCachesRequest {})
        .await
        .expect("list")
        .into_inner();
    assert_eq!(updated.caches[0].max_entries, Some(5000));
}

#[tokio::test]
async fn patch_metrics_sets_enabled_false() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-tracing-prometheus.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    // Verify fixture has metrics enabled
    assert!(
        file_cfg
            .metrics
            .as_ref()
            .and_then(|m| m.enabled)
            .unwrap_or(false),
        "fixture should have metrics enabled"
    );
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-metrics-tracing-prometheus.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );
    let gen0 = snapshots.generation();

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

    let mut metrics = ConduitMetricsClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect metrics");

    // Patch to disable
    let patch = MetricsConfig {
        enabled: Some(false),
        ..Default::default()
    };
    let set = metrics
        .patch_metrics(PatchMetricsRequest {
            metrics: Some(runtime_metrics_to_control(patch)),
        })
        .await
        .expect("patch")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);
    assert!(set.generation > gen0);

    // Verify via snapshot
    let snap = snapshots.load();
    assert_eq!(
        snap.config.metrics.as_ref().and_then(|m| m.enabled),
        Some(false)
    );
}

#[tokio::test]
async fn patch_metrics_plan_knobs_and_get_returns_full_config() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-tracing-prometheus.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-metrics-tracing-prometheus.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

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

    let mut metrics = ConduitMetricsClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect metrics");

    let mut collection = std::collections::HashMap::new();
    collection.insert(
        "timing".into(),
        MetricsCollectEmit {
            collect: Some(true),
            emit: Some(false),
        },
    );
    let patch = MetricsConfig {
        base: "standard".into(),
        categories: Some(MetricsCategories {
            include: vec!["timing".into()],
            exclude: vec!["process".into()],
            include_set: true,
            exclude_set: true,
        }),
        granularity: Some(MetricsGranularity {
            default: "fine".into(),
            overrides: Default::default(),
        }),
        collection,
        ..Default::default()
    };
    let set = metrics
        .patch_metrics(PatchMetricsRequest {
            metrics: Some(runtime_metrics_to_control(patch)),
        })
        .await
        .expect("patch")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);

    let got = metrics
        .get_metrics(GetMetricsRequest {})
        .await
        .expect("get")
        .into_inner()
        .metrics
        .expect("metrics present");
    assert_eq!(got.base, "standard");
    let categories = got.categories.expect("categories");
    assert_eq!(categories.include, vec!["timing".to_string()]);
    assert_eq!(categories.exclude, vec!["process".to_string()]);
    assert_eq!(got.granularity.expect("granularity").default, "fine");
    let timing = got.collection.get("timing").expect("timing collection");
    assert_eq!(timing.collect, Some(true));
    assert_eq!(timing.emit, Some(false));
    // Prometheus listen from fixture preserved by deep-merge
    assert!(
        got.prometheus
            .as_ref()
            .map(|p| !p.listen_address.is_empty())
            .unwrap_or(false),
        "prometheus block should remain from baseline"
    );
}

#[tokio::test]
async fn set_orchestrator_limits_preserves_txn_table_capacity() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    let mut orch = file_cfg.orchestrator.unwrap_or_default();
    orch.txn_table_capacity = 2048;
    orch.max_attempts = 3;
    file_cfg.orchestrator = Some(orch);

    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

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

    let mut orchestrator = ConduitOrchestratorClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let set = orchestrator
        .set_orchestrator_limits(SetOrchestratorLimitsRequest {
            max_attempts: Some(7),
            max_txn_duration_ms: Some(9000),
        })
        .await
        .expect("set")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);

    let got = orchestrator
        .get_orchestrator(GetOrchestratorRequest {})
        .await
        .expect("get")
        .into_inner()
        .orchestrator
        .expect("orchestrator present");
    assert_eq!(got.max_attempts, 7);
    assert_eq!(got.max_txn_duration_ms, 9000);
    assert_eq!(got.txn_table_capacity, 2048);
}

#[tokio::test]
async fn upsert_data_source_and_list() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    file_cfg.data_sources.push(DataSource {
        name: "geo".into(),
        r#type: "csv".into(),
        path: "../data/geo.csv".into(),
        ..Default::default()
    });

    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

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

    let mut ds = ConduitDataSourcesClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let listed = ds
        .list_data_sources(ListDataSourcesRequest {})
        .await
        .expect("list")
        .into_inner();
    assert_eq!(listed.sources.len(), 1);
    assert_eq!(listed.sources[0].name, "geo");

    let upsert = ds
        .upsert_data_source(UpsertDataSourceRequest {
            source: Some(ControlDataSource {
                name: "blocklist".into(),
                r#type: "csv".into(),
                path: "../data/blocklist.csv".into(),
                ..Default::default()
            }),
        })
        .await
        .expect("upsert")
        .into_inner();
    assert!(upsert.ok, "{:?}", upsert.errors);

    let listed = ds
        .list_data_sources(ListDataSourcesRequest {})
        .await
        .expect("list")
        .into_inner();
    assert_eq!(listed.sources.len(), 2);
    assert!(listed.sources.iter().any(|s| s.name == "blocklist"));
}

#[tokio::test]
async fn set_event_sink_filters_on_existing_sink() {
    let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-filters.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let sink_name = file_cfg
        .events
        .as_ref()
        .and_then(|e| e.sinks.first())
        .map(|s| {
            s.name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| s.export_id.clone())
        })
        .expect("fixture has a named sink");

    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-dnstap-filters.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

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

    let mut events = ConduitEventsClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let filters = EventSinkFilters {
        sample_percent: Some(12.5),
        ..Default::default()
    };
    let bytes = filters.encode_to_vec();
    let control_filters =
        ControlEventSinkFilters::decode(bytes.as_slice()).expect("compatible filters");

    let set = events
        .set_event_sink_filters(SetEventSinkFiltersRequest {
            name: sink_name.clone(),
            filters: Some(control_filters),
        })
        .await
        .expect("set-filters")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);

    let snap = snapshots.load();
    let sink = snap
        .config
        .events
        .as_ref()
        .and_then(|e| {
            e.sinks
                .iter()
                .find(|s| s.name.as_deref() == Some(sink_name.as_str()) || s.export_id == sink_name)
        })
        .expect("sink");
    assert_eq!(
        sink.filters.as_ref().and_then(|f| f.sample_percent),
        Some(12.5)
    );
}

#[tokio::test]
async fn cache_memory_shard_count_not_changed_by_policy_hot() {
    // Typed SetCachePolicyHot must not expose memory.shard_count; verify the
    // field remains untouched when only rotate_rrset_on_serve is set.
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    file_cfg.caches.push(CacheInstance {
        name: "answers".into(),
        r#type: "memory".into(),
        max_entries: Some(1000),
        memory: Some(CacheMemoryConfig {
            shard_count: Some(8),
            eviction: Some("passive".into()),
        }),
        rotate_rrset_on_serve: Some(false),
        ..Default::default()
    });

    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

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

    let mut caches = ConduitCachesClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let set = caches
        .set_cache_policy_hot(conduit_proto::control::SetCachePolicyHotRequest {
            name: "answers".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: Some(true),
        })
        .await
        .expect("set-policy")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);

    let cache = snapshots
        .load()
        .config
        .caches
        .iter()
        .find(|c| c.name == "answers")
        .cloned()
        .expect("cache");
    assert_eq!(cache.rotate_rrset_on_serve, Some(true));
    assert_eq!(cache.memory.as_ref().and_then(|m| m.shard_count), Some(8));
    assert_eq!(
        cache.memory.as_ref().and_then(|m| m.eviction.clone()),
        Some("passive".into())
    );
}
