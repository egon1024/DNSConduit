//! Compile events config for runtime snapshots.

use crate::connect_retry::ConnectRetryConfig;
use crate::metrics::SinkMetrics;
use crate::queue::DropPolicy;
use crate::selectors::{
    compile_sample_key_fields, compile_sink_selectors, validate_non_rule_selector_type,
    CompiledSelector, SampleKey, SelectorCompileCtx,
};
use conduit_proto::config::{Config, EventSink, EventSinkFilters, EventsConfig};
use conduit_proto::paths::resolve_config_path;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Metadata field names allowed in `extra_fields`.
pub const EXTRA_FIELD_NAMES: &[&str] = &[
    "pool",
    "backend",
    "attempt_count",
    "txn_id",
    "qname",
    "rcode",
    "tags",
    "client",
    "sink_name",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraField {
    Pool,
    Backend,
    AttemptCount,
    TxnId,
    Qname,
    Rcode,
    Tags,
    Client,
    SinkName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagExportMode {
    All,
    Keys(Vec<String>),
}

impl TagExportMode {
    pub fn wants_tags(&self) -> bool {
        matches!(self, TagExportMode::Keys(keys) if !keys.is_empty())
            || matches!(self, TagExportMode::All)
    }
}

/// Resolved sink identifiers: `name` is the canonical operator/API id; `export_id` is dnstap wire identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkIdentity {
    pub name: String,
    pub export_id: String,
}

/// Resolve `name` and `export_id` with cross-defaulting and legacy compatibility.
///
/// - `name` only → `export_id` defaults to `name`
/// - `export_id` only (legacy) → `name` defaults to `export_id`
/// - both set → use both (may differ)
pub fn resolve_sink_identity(s: &EventSink) -> Result<SinkIdentity, String> {
    let name_opt = s.name.as_ref().filter(|n| !n.is_empty());
    let export_id = s.export_id.trim();
    let export_opt = (!export_id.is_empty()).then_some(export_id);

    match (name_opt, export_opt) {
        (Some(name), Some(export_id)) => Ok(SinkIdentity {
            name: name.clone(),
            export_id: export_id.to_string(),
        }),
        (Some(name), None) => Ok(SinkIdentity {
            name: name.clone(),
            export_id: name.clone(),
        }),
        (None, Some(export_id)) => Ok(SinkIdentity {
            name: export_id.to_string(),
            export_id: export_id.to_string(),
        }),
        (None, None) => Err("requires name or export_id".into()),
    }
}

/// Validate resolved identities are unique across sinks (call from config validation).
pub fn validate_sink_identity_uniqueness(sinks: &[EventSink]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut export_ids: HashMap<String, usize> = HashMap::new();

    for (i, sink) in sinks.iter().enumerate() {
        let Ok(identity) = resolve_sink_identity(sink) else {
            errors.push(format!(
                "events.sinks[{i}] requires name or export_id (canonical operator id and/or dnstap wire identity)"
            ));
            continue;
        };
        if let Some(prev) = names.insert(identity.name.clone(), i) {
            errors.push(format!(
                "events.sinks[{i}].name '{}' duplicates sinks[{prev}]",
                identity.name
            ));
        }
        if let Some(prev) = export_ids.insert(identity.export_id.clone(), i) {
            errors.push(format!(
                "events.sinks[{i}] export_id '{}' duplicates sinks[{prev}]",
                identity.export_id
            ));
        }
    }
    errors
}

pub fn parse_extra_field(name: &str) -> Option<ExtraField> {
    match name {
        "pool" => Some(ExtraField::Pool),
        "backend" => Some(ExtraField::Backend),
        "attempt_count" => Some(ExtraField::AttemptCount),
        "txn_id" => Some(ExtraField::TxnId),
        "qname" => Some(ExtraField::Qname),
        "rcode" => Some(ExtraField::Rcode),
        "tags" => Some(ExtraField::Tags),
        "client" => Some(ExtraField::Client),
        "sink_name" => Some(ExtraField::SinkName),
        _ => None,
    }
}

pub fn parse_extra_fields(names: &[String]) -> Result<Vec<ExtraField>, String> {
    let mut fields = Vec::new();
    for name in names {
        let field = parse_extra_field(name)
            .ok_or_else(|| format!("unknown events extra_fields entry '{name}'"))?;
        fields.push(field);
    }
    Ok(fields)
}

pub fn parse_extra_tags(
    extra_tags: &[String],
    has_tags_field: bool,
) -> Result<TagExportMode, String> {
    if extra_tags.is_empty() {
        return if has_tags_field {
            Ok(TagExportMode::All)
        } else {
            Ok(TagExportMode::Keys(Vec::new()))
        };
    }
    if !has_tags_field {
        return Err("events extra_tags requires 'tags' in extra_fields".into());
    }
    if extra_tags.iter().any(|t| t == "*") {
        if extra_tags.len() > 1 {
            return Err("events extra_tags cannot mix '*' with other keys".into());
        }
        return Ok(TagExportMode::All);
    }
    for tag in extra_tags {
        if tag.is_empty() {
            return Err("events extra_tags entries must not be empty".into());
        }
    }
    Ok(TagExportMode::Keys(extra_tags.to_vec()))
}

#[derive(Debug, Clone)]
pub struct CompiledEvents {
    pub enabled: bool,
    pub queue_depth: usize,
    pub drop_policy: DropPolicy,
    pub sinks: Vec<CompiledSinkInstance>,
    name_to_export_id: HashMap<String, String>,
    export_id_to_name: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CompiledSinkFilters {
    pub selectors: Vec<CompiledSelector>,
    pub tag_required: Option<String>,
    pub sample_percent: f64,
    pub sample_key: SampleKey,
    pub pool: Option<String>,
    pub backend: Option<String>,
}

impl Default for CompiledSinkFilters {
    fn default() -> Self {
        Self {
            selectors: Vec::new(),
            tag_required: None,
            sample_percent: 100.0,
            sample_key: SampleKey::Global,
            pool: None,
            backend: None,
        }
    }
}

pub fn parse_sample_percent(percent: Option<f64>) -> Result<f64, String> {
    match percent {
        None => Ok(100.0),
        Some(p) if (0.0..=100.0).contains(&p) => Ok(p),
        Some(_) => Err("events filters.sample_percent must be in [0, 100]".into()),
    }
}

pub fn parse_sink_filters(
    f: Option<&EventSinkFilters>,
    sink_name: &str,
) -> Result<CompiledSinkFilters, String> {
    let Some(f) = f else {
        return Ok(CompiledSinkFilters {
            sample_percent: 100.0,
            ..Default::default()
        });
    };
    for sel in &f.selectors {
        validate_non_rule_selector_type(sel.r#type.as_str())?;
    }
    if f.pool.as_ref().is_some_and(|p| p.is_empty()) {
        return Err("events filters.pool must not be empty".into());
    }
    if f.backend.as_ref().is_some_and(|b| b.is_empty()) {
        return Err("events filters.backend must not be empty".into());
    }
    let tag_required = f.tag_required.clone().filter(|t| !t.is_empty());
    let ctx = SelectorCompileCtx {
        rule_name: None,
        sink_name: Some(sink_name.to_string()),
    };
    let sample_key = compile_sample_key_fields(
        f.sample_key.as_deref(),
        f.sample_key_from.as_deref(),
        &ctx,
        false,
        true,
    )?;
    Ok(CompiledSinkFilters {
        selectors: compile_sink_selectors(sink_name, &f.selectors),
        tag_required,
        sample_percent: parse_sample_percent(f.sample_percent)?,
        sample_key,
        pool: f.pool.clone().filter(|p| !p.is_empty()),
        backend: f.backend.clone().filter(|b| !b.is_empty()),
    })
}

#[derive(Debug, Clone)]
pub struct CompiledSinkInstance {
    pub emit_query: bool,
    pub emit_response: bool,
    pub emit_retry: bool,
    pub filters: CompiledSinkFilters,
    /// Canonical operator / API / metrics id.
    pub name: String,
    /// Dnstap protobuf `identity` on the wire (defaults to `name` when omitted in config).
    pub export_id: String,
    pub connect_retry: ConnectRetryConfig,
    pub metrics: Arc<SinkMetrics>,
    pub destinations: Vec<Destination>,
    pub extra_fields: Vec<ExtraField>,
    pub tag_export: TagExportMode,
}

impl CompiledEvents {
    pub fn needs_tag_export(&self) -> bool {
        self.sinks
            .iter()
            .any(|s| s.extra_fields.contains(&ExtraField::Tags))
    }

    pub fn export_id_for_name(&self, name: &str) -> Option<&str> {
        self.name_to_export_id.get(name).map(String::as_str)
    }

    pub fn name_for_export_id(&self, export_id: &str) -> Option<&str> {
        self.export_id_to_name.get(export_id).map(String::as_str)
    }

    pub fn sink_by_name(&self, name: &str) -> Option<&CompiledSinkInstance> {
        self.sinks.iter().find(|s| s.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Unix(PathBuf),
    Tcp { host: String, port: u16 },
}

pub fn compile_from_config(cfg: &Config, base_dir: Option<&Path>) -> CompiledEvents {
    let obs = cfg.events.as_ref();
    let (queue_depth, drop_policy, sinks) = match obs {
        Some(o) => (
            default_queue_depth(o),
            parse_drop_policy(o),
            compile_sinks(o, base_dir),
        ),
        None => (8192, DropPolicy::DropOldest, Vec::new()),
    };
    let enabled = !sinks.is_empty();
    let mut name_to_export_id = HashMap::new();
    let mut export_id_to_name = HashMap::new();
    for sink in &sinks {
        name_to_export_id.insert(sink.name.clone(), sink.export_id.clone());
        export_id_to_name.insert(sink.export_id.clone(), sink.name.clone());
    }
    CompiledEvents {
        enabled,
        queue_depth,
        drop_policy,
        sinks,
        name_to_export_id,
        export_id_to_name,
    }
}

fn default_queue_depth(o: &EventsConfig) -> usize {
    if o.queue_depth == 0 {
        8192
    } else {
        o.queue_depth as usize
    }
}

fn parse_drop_policy(o: &EventsConfig) -> DropPolicy {
    DropPolicy::parse(o.drop_policy.as_str()).unwrap_or(DropPolicy::DropOldest)
}

fn compile_sinks(o: &EventsConfig, base_dir: Option<&Path>) -> Vec<CompiledSinkInstance> {
    o.sinks
        .iter()
        .filter_map(|s| compile_one_sink(s, base_dir))
        .collect()
}

pub(crate) fn compile_one_sink(
    s: &EventSink,
    base_dir: Option<&Path>,
) -> Option<CompiledSinkInstance> {
    if s.r#type != "dnstap" || s.destinations.is_empty() {
        return None;
    }
    let identity = resolve_sink_identity(s).ok()?;
    let destinations: Vec<Destination> = s
        .destinations
        .iter()
        .filter_map(|d| parse_destination(d.as_str(), base_dir))
        .collect();
    if destinations.is_empty() {
        return None;
    }
    let emit = normalize_emit(&s.emit);
    let filters = parse_sink_filters(s.filters.as_ref(), &identity.name).ok()?;
    let extra_fields = parse_extra_fields(&s.extra_fields).ok()?;
    let has_tags = extra_fields.contains(&ExtraField::Tags);
    let tag_export = parse_extra_tags(&s.extra_tags, has_tags).ok()?;
    let connect_retry = parse_connect_retry(s).ok()?;
    let metrics = SinkMetrics::new(identity.name.clone());
    Some(CompiledSinkInstance {
        emit_query: emit.query,
        emit_response: emit.response,
        emit_retry: emit.retry,
        filters,
        name: identity.name,
        export_id: identity.export_id,
        connect_retry,
        metrics,
        destinations,
        extra_fields,
        tag_export,
    })
}

pub fn parse_connect_retry(s: &EventSink) -> Result<ConnectRetryConfig, String> {
    let cfg = ConnectRetryConfig::resolve(s.connect_retry.as_ref());
    cfg.validate()?;
    Ok(cfg)
}

struct EmitFlags {
    query: bool,
    response: bool,
    retry: bool,
}

fn normalize_emit(emit: &[String]) -> EmitFlags {
    let mut flags = EmitFlags {
        query: false,
        response: false,
        retry: false,
    };
    for e in emit {
        match e.as_str() {
            "query" => flags.query = true,
            "response" => flags.response = true,
            "retry" => flags.retry = true,
            _ => {}
        }
    }
    if !flags.query && !flags.response && !flags.retry {
        flags.query = true;
        flags.response = true;
    }
    flags
}

pub fn parse_destination(s: &str, base_dir: Option<&Path>) -> Option<Destination> {
    if let Some(path) = s.strip_prefix("unix:") {
        if path.is_empty() {
            return None;
        }
        return Some(Destination::Unix(resolve_config_path(base_dir, path)));
    }
    if let Some(rest) = s.strip_prefix("tcp:") {
        let (host, port) = rest.rsplit_once(':')?;
        let port: u16 = port.parse().ok()?;
        if host.is_empty() {
            return None;
        }
        return Some(Destination::Tcp {
            host: host.to_string(),
            port,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::EventSink;

    #[test]
    fn parse_unix_and_tcp_destinations() {
        assert!(matches!(
            parse_destination("unix:/tmp/x.sock", None),
            Some(Destination::Unix(_))
        ));
        assert!(matches!(
            parse_destination("tcp:127.0.0.1:6000", None),
            Some(Destination::Tcp { .. })
        ));
        assert!(parse_destination("bad", None).is_none());
    }

    #[test]
    fn parse_unix_relative_destination_against_base_dir() {
        let base = Path::new("/etc/conduit");
        let dest = parse_destination("unix:run/dnstap.sock", Some(base)).unwrap();
        assert_eq!(
            dest,
            Destination::Unix(PathBuf::from("/etc/conduit/run/dnstap.sock"))
        );
    }

    #[test]
    fn resolve_name_only_defaults_export_id() {
        let s = EventSink {
            r#type: "dnstap".into(),
            export_id: String::new(),
            name: Some("primary-tap".into()),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            connect_retry: None,
        };
        let id = resolve_sink_identity(&s).unwrap();
        assert_eq!(id.name, "primary-tap");
        assert_eq!(id.export_id, "primary-tap");
    }

    #[test]
    fn resolve_export_id_only_legacy_defaults_name() {
        let s = EventSink {
            r#type: "dnstap".into(),
            export_id: "conduit-dev".into(),
            name: None,
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            connect_retry: None,
        };
        let id = resolve_sink_identity(&s).unwrap();
        assert_eq!(id.name, "conduit-dev");
        assert_eq!(id.export_id, "conduit-dev");
    }

    #[test]
    fn resolve_distinct_name_and_export_id() {
        let s = EventSink {
            r#type: "dnstap".into(),
            export_id: "wire-pod-7".into(),
            name: Some("prod-tap".into()),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec![],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            connect_retry: None,
        };
        let id = resolve_sink_identity(&s).unwrap();
        assert_eq!(id.name, "prod-tap");
        assert_eq!(id.export_id, "wire-pod-7");
    }

    #[test]
    fn compiled_events_lookup_maps() {
        let cfg = Config {
            schema_version: 1,
            events: Some(EventsConfig {
                queue_depth: 128,
                drop_policy: "drop_oldest".into(),
                sinks: vec![
                    EventSink {
                        r#type: "dnstap".into(),
                        name: Some("tap-a".into()),
                        export_id: "wire-a".into(),
                        destinations: vec!["unix:/tmp/a".into()],
                        emit: vec!["query".into()],
                        filters: None,
                        extra_fields: vec![],
                        extra_tags: vec![],
                        connect_retry: None,
                    },
                    EventSink {
                        r#type: "dnstap".into(),
                        name: None,
                        export_id: "legacy-only".into(),
                        destinations: vec!["unix:/tmp/b".into()],
                        emit: vec!["query".into()],
                        filters: None,
                        extra_fields: vec![],
                        extra_tags: vec![],
                        connect_retry: None,
                    },
                ],
            }),
            ..Default::default()
        };
        let compiled = compile_from_config(&cfg, None);
        assert_eq!(compiled.export_id_for_name("tap-a"), Some("wire-a"));
        assert_eq!(compiled.name_for_export_id("wire-a"), Some("tap-a"));
        assert_eq!(
            compiled.name_for_export_id("legacy-only"),
            Some("legacy-only")
        );
        assert!(compiled.sink_by_name("tap-a").is_some());
        assert!(compiled.sink_by_name("missing").is_none());
    }

    #[test]
    fn validate_rejects_duplicate_names() {
        let sinks = vec![
            EventSink {
                r#type: "dnstap".into(),
                name: Some("same".into()),
                export_id: "a".into(),
                destinations: vec!["unix:/tmp/a".into()],
                emit: vec![],
                filters: None,
                extra_fields: vec![],
                extra_tags: vec![],
                connect_retry: None,
            },
            EventSink {
                r#type: "dnstap".into(),
                name: Some("same".into()),
                export_id: "b".into(),
                destinations: vec!["unix:/tmp/b".into()],
                emit: vec![],
                filters: None,
                extra_fields: vec![],
                extra_tags: vec![],
                connect_retry: None,
            },
        ];
        let errs = validate_sink_identity_uniqueness(&sinks);
        assert!(errs.iter().any(|e| e.contains("name 'same'")));
    }
}
