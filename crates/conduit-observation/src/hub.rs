//! Observation hub: fan-out enqueue and per-sink consumer threads.

use crate::compile::{CompiledObservation, CompiledSinkInstance};
use crate::dnstap::DnstapSink;
use crate::event::{EventKind, ObservationEvent};
use crate::queue::SinkQueue;
use crate::sink::ObservationSink;
use crate::view::TxnView;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

struct SinkRuntime {
    queues: Vec<SinkQueue>,
    _thread: JoinHandle<()>,
}

/// Shared hub for dataplane workers.
pub struct ObservationHub {
    enabled: bool,
    drops: Arc<AtomicU64>,
    sinks: Vec<SinkRuntime>,
}

impl ObservationHub {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            drops: Arc::new(AtomicU64::new(0)),
            sinks: Vec::new(),
        }
    }

    pub fn from_compiled(compiled: &CompiledObservation) -> Self {
        if !compiled.enabled {
            return Self::disabled();
        }
        let drops = Arc::new(AtomicU64::new(0));
        let mut sinks = Vec::new();
        for instance in &compiled.sinks {
            let queue = SinkQueue::new(compiled.queue_depth, compiled.drop_policy);
            let rx = queue.receiver();
            let compiled_instance = instance.clone();
            let thread = thread::Builder::new()
                .name(format!("obs-{}", instance.export_id))
                .spawn(move || {
                    DnstapSink::new(compiled_instance).run(rx);
                })
                .expect("spawn observation sink thread");
            sinks.push(SinkRuntime {
                queues: vec![queue],
                _thread: thread,
            });
        }
        Self {
            enabled: true,
            drops,
            sinks,
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

    pub fn try_enqueue_query(
        &self,
        view: TxnView<'_>,
        compiled: &CompiledObservation,
        tag_has: impl Fn(&str) -> bool,
    ) {
        if !self.enabled || view.qname.is_none() || view.query_wire.is_empty() {
            return;
        }
        self.try_enqueue_kind(
            &view,
            compiled,
            &tag_has,
            EventKind::Query,
            view.query_wire.to_vec(),
        );
    }

    pub fn try_enqueue_response(
        &self,
        view: TxnView<'_>,
        compiled: &CompiledObservation,
        tag_has: impl Fn(&str) -> bool,
    ) {
        let Some(wire) = view.response_wire else {
            return;
        };
        if !self.enabled || wire.is_empty() {
            return;
        }
        self.try_enqueue_kind(
            &view,
            compiled,
            &tag_has,
            EventKind::Response,
            wire.to_vec(),
        );
    }

    pub fn try_enqueue_retry(
        &self,
        view: TxnView<'_>,
        compiled: &CompiledObservation,
        tag_has: impl Fn(&str) -> bool,
    ) {
        if !self.enabled || view.attempt_count <= 1 {
            return;
        }
        self.try_enqueue_kind(
            &view,
            compiled,
            &tag_has,
            EventKind::Retry,
            view.query_wire.to_vec(),
        );
    }

    fn try_enqueue_kind(
        &self,
        view: &TxnView<'_>,
        compiled: &CompiledObservation,
        tag_has: &dyn Fn(&str) -> bool,
        kind: EventKind,
        wire: Vec<u8>,
    ) {
        for (idx, sink_rt) in self.sinks.iter().enumerate() {
            let Some(instance) = compiled.sinks.get(idx) else {
                continue;
            };
            if !emit_allowed(instance, kind) {
                continue;
            }
            if let Some(ref tag) = instance.tag_required {
                if !tag_has(tag) {
                    continue;
                }
            }
            let event = ObservationEvent {
                kind,
                txn_id: view.txn_id,
                client_addr: view.client_addr,
                protocol_udp: view.protocol_udp,
                wire: wire.clone(),
                attempt_count: view.attempt_count,
            };
            if sink_rt.queues[0].try_enqueue(event) {
                self.drops.fetch_add(1, Ordering::Relaxed);
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
    use crate::view::TxnView;
    use crate::DropPolicy;
    use conduit_proto::config::{
        Config, ObservationConfig, ObservationSink, ObservationSinkFilters,
    };
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    fn test_config(sinks: Vec<ObservationSink>) -> Config {
        Config {
            schema_version: 1,
            observation: Some(ObservationConfig {
                queue_depth: 2,
                drop_policy: "drop_newest".into(),
                sinks,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn noop_hub_does_not_allocate_consumer_threads() {
        let hub = ObservationHub::from_compiled(&compile_from_config(&Config {
            schema_version: 1,
            observation: Some(ObservationConfig {
                queue_depth: 4096,
                drop_policy: "drop_oldest".into(),
                sinks: vec![],
            }),
            ..Default::default()
        }));
        assert!(!hub.enabled());
        assert_eq!(hub.consumer_count(), 0);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 53);
        hub.try_enqueue_query(
            TxnView {
                txn_id: 1,
                client_addr: addr,
                protocol_udp: true,
                qname: Some("example.com"),
                query_wire: &[1, 2, 3],
                response_wire: None,
                attempt_count: 1,
            },
            &CompiledObservation {
                enabled: false,
                queue_depth: 0,
                drop_policy: DropPolicy::DropOldest,
                sinks: vec![],
            },
            |_| true,
        );
        assert_eq!(hub.dropped_total(), 0);
    }

    #[test]
    fn drop_counter_increments_on_full_queue() {
        let cfg = test_config(vec![ObservationSink {
            r#type: "dnstap".into(),
            export_id: "test".into(),
            destinations: vec!["unix:/nonexistent-dnstap.sock".into()],
            emit: vec!["query".into()],
            filters: None,
        }]);
        let compiled = compile_from_config(&cfg);
        let hub = ObservationHub::from_compiled(&compiled);
        assert_eq!(hub.consumer_count(), 1);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 53);
        let view = |id| TxnView {
            txn_id: id,
            client_addr: addr,
            protocol_udp: true,
            qname: Some("x"),
            query_wire: &[0u8; 8],
            response_wire: None,
            attempt_count: 1,
        };
        hub.try_enqueue_query(view(1), &compiled, |_| true);
        hub.try_enqueue_query(view(2), &compiled, |_| true);
        hub.try_enqueue_query(view(3), &compiled, |_| true);
        std::thread::sleep(Duration::from_millis(20));
        assert!(hub.dropped_total() >= 1);
    }

    #[test]
    fn tag_required_compiled_from_filters() {
        let instance = crate::compile::compile_one_sink(&ObservationSink {
            r#type: "dnstap".into(),
            export_id: "x".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec!["query".into()],
            filters: Some(ObservationSinkFilters {
                tag_required: Some("vip".into()),
            }),
        })
        .unwrap();
        assert_eq!(instance.tag_required.as_deref(), Some("vip"));
    }
}
