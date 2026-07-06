//! Observation hub: fan-out enqueue and per-sink consumer threads.

use crate::compile::{CompiledEvents, CompiledSinkInstance};
use crate::dnstap::DnstapSink;
use crate::event::{EventKind, ExportEvent};
use crate::extra::build_extra_json;
use crate::filters::sink_event_matches;
use crate::metrics::{SinkMetrics, SinkMetricsSnapshot};
use crate::queue::SinkQueue;
use crate::view::TxnView;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

struct SinkRuntime {
    metrics: Arc<SinkMetrics>,
    queues: Vec<SinkQueue>,
    thread: JoinHandle<()>,
}

/// Shared hub for dataplane workers.
pub struct EventHub {
    enabled: bool,
    drops: Arc<AtomicU64>,
    sink_metrics: Vec<Arc<SinkMetrics>>,
    sink_shutdown: Arc<AtomicBool>,
    sinks: Vec<SinkRuntime>,
}

impl EventHub {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            drops: Arc::new(AtomicU64::new(0)),
            sink_metrics: Vec::new(),
            sink_shutdown: Arc::new(AtomicBool::new(false)),
            sinks: Vec::new(),
        }
    }

    pub fn from_compiled(compiled: &CompiledEvents) -> Self {
        if !compiled.enabled {
            return Self::disabled();
        }
        let drops = Arc::new(AtomicU64::new(0));
        let sink_shutdown = Arc::new(AtomicBool::new(false));
        let mut sinks = Vec::new();
        let mut sink_metrics = Vec::new();
        for instance in &compiled.sinks {
            let queue = SinkQueue::new(compiled.queue_depth, compiled.drop_policy);
            let rx = queue.receiver();
            let compiled_instance = instance.clone();
            let metrics = instance.metrics.clone();
            let worker_shutdown = sink_shutdown.clone();
            let thread = thread::Builder::new()
                .name(format!("obs-{}", instance.name))
                .spawn(move || {
                    DnstapSink::new(compiled_instance).run_with_shutdown(rx, worker_shutdown);
                })
                .expect("spawn observation sink thread");
            sink_metrics.push(metrics.clone());
            sinks.push(SinkRuntime {
                metrics,
                queues: vec![queue],
                thread,
            });
        }
        Self {
            enabled: true,
            drops,
            sink_metrics,
            sink_shutdown,
            sinks,
        }
    }

    /// Stop sink consumer threads: signal workers, drop queue senders, join threads.
    pub fn shutdown(mut self) {
        self.sink_shutdown.store(true, Ordering::Relaxed);
        for sink in self.sinks.drain(..) {
            drop(sink.queues);
            if let Err(e) = sink.thread.join() {
                std::panic::resume_unwind(e);
            }
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn consumer_count(&self) -> usize {
        self.sinks.len()
    }

    pub fn dropped_total(&self) -> u64 {
        self.drops.load(Ordering::Relaxed)
    }

    pub fn sink_metrics_snapshot(&self) -> Vec<SinkMetricsSnapshot> {
        self.sink_metrics.iter().map(|m| m.snapshot()).collect()
    }

    pub fn try_enqueue_query(
        &self,
        view: TxnView<'_>,
        compiled: &CompiledEvents,
        tag_has: impl Fn(&str) -> bool,
    ) {
        if !self.enabled || view.qname.is_none() || view.query_wire.is_empty() {
            return;
        }
        self.try_enqueue_kind(&view, compiled, &tag_has, EventKind::Query, view.query_wire);
    }

    pub fn try_enqueue_response(
        &self,
        view: TxnView<'_>,
        compiled: &CompiledEvents,
        tag_has: impl Fn(&str) -> bool,
    ) {
        let Some(wire) = view.response_wire else {
            return;
        };
        if !self.enabled || wire.is_empty() {
            return;
        }
        self.try_enqueue_kind(&view, compiled, &tag_has, EventKind::Response, wire);
    }

    pub fn try_enqueue_retry(
        &self,
        view: TxnView<'_>,
        compiled: &CompiledEvents,
        tag_has: impl Fn(&str) -> bool,
    ) {
        if !self.enabled || view.attempt_count <= 1 {
            return;
        }
        self.try_enqueue_kind(&view, compiled, &tag_has, EventKind::Retry, view.query_wire);
    }

    fn try_enqueue_kind(
        &self,
        view: &TxnView<'_>,
        compiled: &CompiledEvents,
        tag_has: &dyn Fn(&str) -> bool,
        kind: EventKind,
        wire: &[u8],
    ) {
        for (idx, sink_rt) in self.sinks.iter().enumerate() {
            let Some(instance) = compiled.sinks.get(idx) else {
                continue;
            };
            if !emit_allowed(instance, kind) {
                continue;
            }
            if !sink_event_matches(&instance.filters, kind, view, tag_has) {
                continue;
            }
            let extra = build_extra_json(instance, &view.extra);
            let event = ExportEvent {
                kind,
                txn_id: view.txn_id,
                client_addr: view.client_addr,
                protocol_udp: view.protocol_udp,
                wire: wire.to_vec(),
                attempt_count: view.attempt_count,
                extra,
            };
            if sink_rt.queues[0].try_enqueue(event) {
                sink_rt.metrics.record_queue_dropped();
                self.drops.fetch_add(1, Ordering::Relaxed);
            } else {
                sink_rt.metrics.record_enqueued(kind);
            }
        }
    }
}

fn emit_allowed(instance: &CompiledSinkInstance, kind: EventKind) -> bool {
    match kind {
        EventKind::Query => instance.emit_query,
        EventKind::Response => instance.emit_response,
        EventKind::Retry => instance.emit_retry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_from_config;
    use crate::view::{TxnExtraSource, TxnView};
    use conduit_proto::config::{Config, EventSink, EventSinkFilters, EventsConfig};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    fn test_config(sinks: Vec<EventSink>) -> Config {
        Config {
            schema_version: 1,
            events: Some(EventsConfig {
                queue_depth: 2,
                drop_policy: "drop_newest".into(),
                sinks,
            }),
            ..Default::default()
        }
    }

    fn sample_view(id: u64) -> TxnView<'static> {
        TxnView {
            txn_id: id,
            global_query_index: id,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 53),
            protocol_udp: true,
            qname: Some("x"),
            qtype: Some(1),
            rcode: None,
            qclass: None,
            opcode: None,
            edns_option_codes: &[],
            qtype_label: Some("A".into()),
            query_wire: &[0u8; 8],
            response_wire: None,
            attempt_count: 1,
            answer_source: None,
            cache_instance: None,
            extra: TxnExtraSource::default(),
        }
    }

    #[test]
    fn disabled_hub_shutdown_is_noop() {
        EventHub::disabled().shutdown();
    }

    #[test]
    fn shutdown_joins_sink_threads() {
        let cfg = test_config(vec![EventSink {
            r#type: "dnstap".into(),
            export_id: "shutdown-test".into(),
            destinations: vec!["unix:/nonexistent-dnstap-shutdown.sock".into()],
            emit: vec!["query".into()],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry: None,
        }]);
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        assert_eq!(hub.consumer_count(), 1);
        hub.shutdown();
    }

    #[test]
    fn noop_hub_does_not_allocate_consumer_threads() {
        let hub = EventHub::from_compiled(&compile_from_config(
            &Config {
                schema_version: 1,
                events: Some(EventsConfig {
                    queue_depth: 4096,
                    drop_policy: "drop_oldest".into(),
                    sinks: vec![],
                }),
                ..Default::default()
            },
            None,
        ));
        assert!(!hub.enabled());
        assert_eq!(hub.consumer_count(), 0);
        let disabled = compile_from_config(
            &Config {
                schema_version: 1,
                events: None,
                ..Default::default()
            },
            None,
        );
        hub.try_enqueue_query(sample_view(1), &disabled, |_| true);
        assert_eq!(hub.dropped_total(), 0);
        assert!(hub.sink_metrics_snapshot().is_empty());
    }

    #[test]
    fn drop_counter_increments_on_full_queue() {
        let cfg = test_config(vec![EventSink {
            r#type: "dnstap".into(),
            export_id: "test".into(),
            destinations: vec!["unix:/nonexistent-dnstap.sock".into()],
            emit: vec!["query".into()],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry: None,
        }]);
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        assert_eq!(hub.consumer_count(), 1);
        hub.try_enqueue_query(sample_view(1), &compiled, |_| true);
        hub.try_enqueue_query(sample_view(2), &compiled, |_| true);
        hub.try_enqueue_query(sample_view(3), &compiled, |_| true);
        std::thread::sleep(Duration::from_millis(20));
        assert!(hub.dropped_total() >= 1);
        let snap = hub.sink_metrics_snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "test");
        assert!(snap[0].queue_dropped >= 1);
    }

    #[test]
    fn canonical_name_when_name_and_export_id_differ() {
        let cfg = test_config(vec![EventSink {
            r#type: "dnstap".into(),
            export_id: "wire-id".into(),
            name: Some("tap-primary".into()),
            destinations: vec!["unix:/nonexistent.sock".into()],
            emit: vec!["query".into()],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            connect_retry: None,
        }]);
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        hub.try_enqueue_query(sample_view(1), &compiled, |_| true);
        let snap = hub.sink_metrics_snapshot();
        assert_eq!(snap[0].name, "tap-primary");
        assert_eq!(snap[0].enqueued_query, 1);
    }

    #[test]
    fn fan_out_increments_both_sinks() {
        let cfg = test_config(vec![
            EventSink {
                r#type: "dnstap".into(),
                export_id: "a".into(),
                destinations: vec!["unix:/nonexistent-a.sock".into()],
                emit: vec!["query".into()],
                filters: None,
                extra_fields: vec![],
                extra_tags: vec![],
                name: None,
                connect_retry: None,
            },
            EventSink {
                r#type: "dnstap".into(),
                export_id: "b".into(),
                destinations: vec!["unix:/nonexistent-b.sock".into()],
                emit: vec!["query".into()],
                filters: None,
                extra_fields: vec![],
                extra_tags: vec![],
                name: None,
                connect_retry: None,
            },
        ]);
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        hub.try_enqueue_query(sample_view(1), &compiled, |_| true);
        let snap = hub.sink_metrics_snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].enqueued_query, 1);
        assert_eq!(snap[1].enqueued_query, 1);
    }

    #[test]
    fn partial_drop_only_hits_sink_that_receives_events() {
        let cfg = Config {
            schema_version: 1,
            events: Some(EventsConfig {
                queue_depth: 1,
                drop_policy: "drop_newest".into(),
                sinks: vec![
                    EventSink {
                        r#type: "dnstap".into(),
                        export_id: "responses".into(),
                        destinations: vec!["unix:/nonexistent-responses.sock".into()],
                        emit: vec!["response".into()],
                        filters: None,
                        extra_fields: vec![],
                        extra_tags: vec![],
                        name: None,
                        connect_retry: None,
                    },
                    EventSink {
                        r#type: "dnstap".into(),
                        export_id: "queries".into(),
                        destinations: vec!["unix:/nonexistent-queries.sock".into()],
                        emit: vec!["query".into()],
                        filters: None,
                        extra_fields: vec![],
                        extra_tags: vec![],
                        name: None,
                        connect_retry: None,
                    },
                ],
            }),
            ..Default::default()
        };
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        hub.try_enqueue_query(sample_view(1), &compiled, |_| true);
        hub.try_enqueue_query(sample_view(2), &compiled, |_| true);
        hub.try_enqueue_query(sample_view(3), &compiled, |_| true);
        let snap = hub.sink_metrics_snapshot();
        let responses = snap.iter().find(|s| s.name == "responses").unwrap();
        let queries = snap.iter().find(|s| s.name == "queries").unwrap();
        assert_eq!(responses.enqueued_query, 0);
        assert_eq!(responses.queue_dropped, 0);
        assert_eq!(queries.enqueued_query, 1);
        assert_eq!(queries.queue_dropped, 2);
    }

    #[test]
    fn tag_required_compiled_from_filters() {
        let instance = crate::compile::compile_one_sink(
            &EventSink {
                r#type: "dnstap".into(),
                export_id: "x".into(),
                destinations: vec!["unix:/tmp/x".into()],
                emit: vec!["query".into()],
                filters: Some(EventSinkFilters {
                    tag_required: Some("vip".into()),
                    selectors: vec![],
                    sample_percent: None,
                    sample_key: None,
                    sample_key_from: None,
                    pool: None,
                    backend: None,
                }),
                extra_fields: vec![],
                extra_tags: vec![],
                name: None,
                connect_retry: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(instance.filters.tag_required.as_deref(), Some("vip"));
    }

    #[test]
    fn selector_filter_skips_enqueue() {
        use conduit_proto::config::Selector;

        let cfg = test_config(vec![EventSink {
            r#type: "dnstap".into(),
            export_id: "sel".into(),
            destinations: vec!["unix:/nonexistent.sock".into()],
            emit: vec!["query".into()],
            filters: Some(EventSinkFilters {
                tag_required: None,
                selectors: vec![Selector {
                    r#type: "qtype".into(),
                    value: "AAAA".into(),
                    key: None,
                    key_from: None,
                }],
                sample_percent: None,
                sample_key: None,
                sample_key_from: None,
                pool: None,
                backend: None,
            }),
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry: None,
        }]);
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        hub.try_enqueue_query(sample_view(1), &compiled, |_| true);
        let snap = hub.sink_metrics_snapshot();
        assert_eq!(snap[0].enqueued_query, 0);
    }

    #[test]
    fn sample_key_from_sink_name_compiles() {
        let instance = crate::compile::compile_one_sink(
            &EventSink {
                r#type: "dnstap".into(),
                export_id: "x".into(),
                name: Some("my-sink".into()),
                destinations: vec!["unix:/tmp/x".into()],
                emit: vec!["query".into()],
                filters: Some(EventSinkFilters {
                    tag_required: None,
                    selectors: vec![],
                    sample_percent: Some(100.0),
                    sample_key: None,
                    sample_key_from: Some("sink_name".into()),
                    pool: None,
                    backend: None,
                }),
                extra_fields: vec![],
                extra_tags: vec![],
                connect_retry: None,
            },
            None,
        )
        .unwrap();
        assert!(matches!(
            instance.filters.sample_key,
            crate::selectors::SampleKey::Literal(ref k) if k == "my-sink"
        ));
    }

    #[test]
    fn sample_percent_reduces_enqueued_volume() {
        let cfg = test_config(vec![EventSink {
            r#type: "dnstap".into(),
            export_id: "sample".into(),
            destinations: vec!["unix:/nonexistent.sock".into()],
            emit: vec!["query".into()],
            filters: Some(EventSinkFilters {
                tag_required: None,
                selectors: vec![],
                sample_percent: Some(1.0),
                sample_key: None,
                sample_key_from: None,
                pool: None,
                backend: None,
            }),
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry: None,
        }]);
        let compiled = compile_from_config(&cfg, None);
        let hub = EventHub::from_compiled(&compiled);
        for id in 1..=200u64 {
            let mut view = sample_view(id);
            view.qtype_label = Some("A".into());
            hub.try_enqueue_query(view, &compiled, |_| true);
        }
        let enqueued: u64 = hub
            .sink_metrics_snapshot()
            .iter()
            .map(|s| s.enqueued_query)
            .sum();
        assert!(enqueued < 200, "expected sampling to drop most events");
        assert!(enqueued > 0, "expected some events at 1% sample rate");
    }
}
