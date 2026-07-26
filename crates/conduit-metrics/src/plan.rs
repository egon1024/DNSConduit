//! Metrics configurability: composable `MetricsPlan` compiled from
//! `enabled` + `base` + `categories` + `granularity` + `collection` +
//! `event_export` (design decisions §1-§4, §9, §12).
//!
//! This module is the **source of truth** for category resolution,
//! collect/emit validation, and the maintainer preset tables. It intentionally
//! does not know about Prometheus/OTLP wiring — [`crate::builtin::BuiltinRegistry`]
//! consumes a [`CompiledMetricsPlan`] to decide what to register and record.

use conduit_proto::config::MetricsConfig;
use std::collections::{BTreeMap, BTreeSet};

/// Dataplane metric categories (design §12 day-one membership table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MetricCategory {
    Volume,
    Failures,
    Lookup,
    Timing,
    CacheDetail,
    ForwardDetail,
    Health,
    Runtime,
    Topology,
    Process,
    Meta,
}

impl MetricCategory {
    pub const ALL: [MetricCategory; 11] = [
        MetricCategory::Volume,
        MetricCategory::Failures,
        MetricCategory::Lookup,
        MetricCategory::Timing,
        MetricCategory::CacheDetail,
        MetricCategory::ForwardDetail,
        MetricCategory::Health,
        MetricCategory::Runtime,
        MetricCategory::Topology,
        MetricCategory::Process,
        MetricCategory::Meta,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MetricCategory::Volume => "volume",
            MetricCategory::Failures => "failures",
            MetricCategory::Lookup => "lookup",
            MetricCategory::Timing => "timing",
            MetricCategory::CacheDetail => "cache_detail",
            MetricCategory::ForwardDetail => "forward_detail",
            MetricCategory::Health => "health",
            MetricCategory::Runtime => "runtime",
            MetricCategory::Topology => "topology",
            MetricCategory::Process => "process",
            MetricCategory::Meta => "meta",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        MetricCategory::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// `metrics.base` preset (design §Decisions 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsBase {
    None,
    Minimal,
    Standard,
}

impl MetricsBase {
    pub fn as_str(self) -> &'static str {
        match self {
            MetricsBase::None => "none",
            MetricsBase::Minimal => "minimal",
            MetricsBase::Standard => "standard",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "none" => Some(MetricsBase::None),
            "minimal" => Some(MetricsBase::Minimal),
            "standard" => Some(MetricsBase::Standard),
            _ => None,
        }
    }
}

/// `metrics.granularity.default` preset (design §Decisions 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Coarse,
    Balanced,
    Fine,
}

impl Granularity {
    pub fn as_str(self) -> &'static str {
        match self {
            Granularity::Coarse => "coarse",
            Granularity::Balanced => "balanced",
            Granularity::Fine => "fine",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "coarse" => Some(Granularity::Coarse),
            "balanced" => Some(Granularity::Balanced),
            "fine" => Some(Granularity::Fine),
            _ => None,
        }
    }
}

/// Responses `rcode` label value mode (design §Decisions 5 — orthogonal to
/// dimension lists). Coarse = class buckets (`NOERROR`/`NXDOMAIN`/…/`OTHER`);
/// Iana = per-code names (today's `profile: full` behaviour).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsesRcodeBucketing {
    Coarse,
    Iana,
}

impl ResponsesRcodeBucketing {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponsesRcodeBucketing::Coarse => "coarse",
            ResponsesRcodeBucketing::Iana => "iana",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "coarse" => Some(ResponsesRcodeBucketing::Coarse),
            "iana" => Some(ResponsesRcodeBucketing::Iana),
            _ => None,
        }
    }
}

/// Default rcode bucketing for a granularity preset (design §12).
pub fn default_responses_rcode(g: Granularity) -> ResponsesRcodeBucketing {
    match g {
        Granularity::Coarse | Granularity::Balanced => ResponsesRcodeBucketing::Coarse,
        Granularity::Fine => ResponsesRcodeBucketing::Iana,
    }
}

/// Known granularity families (closed set for overrides + preset expansion).
pub const GRANULARITY_FAMILIES: &[&str] = &[
    "volume",
    "responses",
    "timing",
    "forward_failures",
    "acl",
    "cache_lookup",
];

/// Maintainer table: closed dimension vocabulary per family, in canonical
/// order (used to normalize operator-provided lists at compile time).
pub fn family_allowed_dimensions(family: &str) -> Option<&'static [&'static str]> {
    match family {
        "volume" => Some(&["listener", "protocol", "qtype", "qclass", "ip_family"]),
        "responses" => Some(&[
            "listener",
            "protocol",
            "rcode",
            "ip_family",
            "answer_source",
        ]),
        "timing" => Some(&["pool", "backend"]),
        "forward_failures" => Some(&["pool", "backend", "reason"]),
        "acl" => Some(&["tier", "action", "listener", "ip_family"]),
        "cache_lookup" => Some(&["cache", "profile", "result"]),
        _ => None,
    }
}

/// Maintainer preset: expand `granularity.default` to a dimension list for
/// `family` (design §Decisions 5). `fine` reproduces today's `profile: full`
/// label schemas exactly (1.x compatibility).
pub fn preset_family_dimensions(
    family: &str,
    granularity: Granularity,
) -> Option<&'static [&'static str]> {
    match family {
        "volume" => Some(match granularity {
            Granularity::Coarse => &["listener", "protocol"],
            Granularity::Balanced => &["listener", "protocol", "qtype"],
            Granularity::Fine => &["listener", "protocol", "qtype", "qclass", "ip_family"],
        }),
        "responses" => Some(match granularity {
            Granularity::Coarse | Granularity::Balanced => {
                &["listener", "protocol", "rcode", "answer_source"]
            }
            Granularity::Fine => &[
                "listener",
                "protocol",
                "rcode",
                "ip_family",
                "answer_source",
            ],
        }),
        "timing" => Some(match granularity {
            Granularity::Coarse => &[],
            Granularity::Balanced => &["pool"],
            Granularity::Fine => &["pool", "backend"],
        }),
        "forward_failures" => Some(match granularity {
            // Keep pool+backend on coarse so `base: minimal` matches today's
            // `profile: minimal` forward_errors series identity (1.x compat).
            Granularity::Coarse => &["pool", "backend", "reason"],
            Granularity::Balanced => &["pool", "reason"],
            Granularity::Fine => &["pool", "backend", "reason"],
        }),
        "acl" => Some(match granularity {
            Granularity::Coarse | Granularity::Balanced => &["tier", "action", "listener"],
            Granularity::Fine => &["tier", "action", "listener", "ip_family"],
        }),
        "cache_lookup" => Some(&["cache", "profile", "result"]),
        _ => None,
    }
}

/// Normalize an operator-supplied dimension list into canonical vocabulary
/// order. Unknown dimensions must be rejected by the caller before this.
fn normalize_dimensions(allowed: &[&str], provided: &[String]) -> Vec<String> {
    allowed
        .iter()
        .filter(|dim| provided.iter().any(|d| d == *dim))
        .map(|dim| dim.to_string())
        .collect()
}

/// Maintainer table: categories in `base: minimal` (design §12).
///
/// **Intentional 1.x expansion:** `health` is included here even though
/// today's `profile: minimal` omitted health series.
pub const MINIMAL_CATEGORIES: &[MetricCategory] = &[
    MetricCategory::Volume,
    MetricCategory::Failures,
    MetricCategory::Lookup,
    MetricCategory::Health,
    MetricCategory::Topology,
    MetricCategory::Meta,
];

/// Maintainer table: categories in `base: standard` (design §12). Reproduces
/// today's `profile: full` category membership exactly (1.x compatibility).
pub const STANDARD_CATEGORIES: &[MetricCategory] = &[
    MetricCategory::Volume,
    MetricCategory::Failures,
    MetricCategory::Lookup,
    MetricCategory::Timing,
    MetricCategory::CacheDetail,
    MetricCategory::ForwardDetail,
    MetricCategory::Health,
    MetricCategory::Runtime,
    MetricCategory::Topology,
    MetricCategory::Process,
    MetricCategory::Meta,
];

/// `C₀ = expand(base)` (design §Decisions 2-3).
pub fn expand_base(base: MetricsBase) -> BTreeSet<MetricCategory> {
    match base {
        MetricsBase::None => BTreeSet::new(),
        MetricsBase::Minimal => MINIMAL_CATEGORIES.iter().copied().collect(),
        MetricsBase::Standard => STANDARD_CATEGORIES.iter().copied().collect(),
    }
}

/// `base: minimal` → `coarse`; `base: standard` → `fine` (design §Decisions 2, §12).
pub fn default_granularity_for_base(base: MetricsBase) -> Granularity {
    match base {
        MetricsBase::Minimal => Granularity::Coarse,
        MetricsBase::Standard | MetricsBase::None => Granularity::Fine,
    }
}

/// Per-user-metric collect/emit flags and optional HELP (design §Decisions 4, §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMetricMode {
    pub collect: bool,
    pub emit: bool,
    /// Prometheus HELP / OTel description; empty → default at export time.
    pub help: String,
}

/// Compiled, generation-scoped metrics plan (design §Runtime architecture).
///
/// Attached to [`crate::CompiledMetrics`] on `RuntimeSnapshot.metrics`.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledMetricsPlan {
    pub enabled: bool,
    pub base: MetricsBase,
    pub categories: BTreeSet<MetricCategory>,
    pub collect: BTreeMap<MetricCategory, bool>,
    pub emit: BTreeMap<MetricCategory, bool>,
    pub granularity_default: Granularity,
    /// Validated per-family dimension overrides as supplied (before preset
    /// merge). Prefer [`Self::dimensions_for`] for the resolved list.
    pub granularity_overrides: BTreeMap<String, Vec<String>>,
    /// Fully resolved dimension lists per family after preset expansion and
    /// per-family overrides (design §Decisions 5).
    pub family_dimensions: BTreeMap<String, Vec<String>>,
    /// Responses `rcode` value mode (orthogonal to dimension lists).
    pub responses_rcode: ResponsesRcodeBucketing,
    pub event_export_collect: bool,
    pub event_export_emit: bool,
    /// Explicit `metrics.user_metrics[]` overrides (name without `conduit_user_`
    /// prefix). Metrics not listed use the default `export: full` semantics
    /// via [`Self::user_collect_for`] / [`Self::user_emit_for`].
    pub user_metrics: BTreeMap<String, UserMetricMode>,
}

impl CompiledMetricsPlan {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            base: MetricsBase::None,
            categories: BTreeSet::new(),
            collect: BTreeMap::new(),
            emit: BTreeMap::new(),
            granularity_default: Granularity::Fine,
            granularity_overrides: BTreeMap::new(),
            family_dimensions: BTreeMap::new(),
            responses_rcode: ResponsesRcodeBucketing::Iana,
            event_export_collect: false,
            event_export_emit: false,
            user_metrics: BTreeMap::new(),
        }
    }

    /// Resolved label dimensions for `family` (empty slice when unknown).
    pub fn dimensions_for(&self, family: &str) -> &[String] {
        self.family_dimensions
            .get(family)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Whether `family`'s resolved schema includes `dim`.
    pub fn has_dimension(&self, family: &str, dim: &str) -> bool {
        self.dimensions_for(family).iter().any(|d| d == dim)
    }

    /// Host/hot-path collect mask for `cat` (design §Decisions 4).
    pub fn collect_for(&self, cat: MetricCategory) -> bool {
        self.enabled && self.categories.contains(&cat) && *self.collect.get(&cat).unwrap_or(&true)
    }

    /// Prometheus/OTLP emit mask for `cat` (design §Decisions 4).
    pub fn emit_for(&self, cat: MetricCategory) -> bool {
        self.enabled && self.categories.contains(&cat) && *self.emit.get(&cat).unwrap_or(&true)
    }

    /// Whether the resolved plan is "standard-tier" for legacy
    /// `user_metrics[].export: full` (fine granularity ≡ today's `profile: full`).
    pub fn user_metrics_standard_tier(&self) -> bool {
        self.enabled && self.granularity_default == Granularity::Fine
    }

    /// Host collect mask for a Rhai user metric (bare name, no prefix).
    pub fn user_collect_for(&self, name: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(m) = self.user_metrics.get(name) {
            return m.collect;
        }
        // Default = deprecated `export: full`: collect only on standard-tier plans.
        self.user_metrics_standard_tier()
    }

    /// Prometheus/OTLP emit mask for a Rhai user metric (bare name, no prefix).
    pub fn user_emit_for(&self, name: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(m) = self.user_metrics.get(name) {
            return m.emit;
        }
        self.user_metrics_standard_tier()
    }

    /// Prometheus HELP / OTel description for a Rhai user metric (bare name).
    /// Empty when unset (export uses the default HELP string).
    pub fn user_help_for(&self, name: &str) -> &str {
        self.user_metrics
            .get(name)
            .map(|m| m.help.as_str())
            .unwrap_or("")
    }

    /// Bare name → non-empty HELP overrides for [`crate::UserRegistry`].
    pub fn user_helps(&self) -> std::collections::HashMap<String, String> {
        self.user_metrics
            .iter()
            .filter(|(_, m)| !m.help.is_empty())
            .map(|(n, m)| (n.clone(), m.help.clone()))
            .collect()
    }

    /// Whether a gathered Prometheus family name should be exported under
    /// this plan's emit mask (built-ins by category; `conduit_user_*` by
    /// user-metric mode; `conduit_events_*` by `event_export_emit`).
    pub fn emits_family(&self, family_name: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if family_name.starts_with("conduit_events_") {
            return self.event_export_emit;
        }
        if let Some(bare) = family_name.strip_prefix("conduit_user_") {
            return self.user_emit_for_sanitized(bare);
        }
        match builtin_metric_category(family_name) {
            Some(cat) => self.emit_for(cat),
            // Unknown family: keep exporting so we do not silently drop
            // series during incremental registry growth.
            None => true,
        }
    }

    /// Emit mask for a Prometheus family bare name (`conduit_user_<bare>`).
    fn user_emit_for_sanitized(&self, sanitized_bare: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(m) = self.user_metrics.get(sanitized_bare) {
            return m.emit;
        }
        for (name, mode) in &self.user_metrics {
            if sanitize_user_metric_name(name) == sanitized_bare {
                return mode.emit;
            }
        }
        self.user_metrics_standard_tier()
    }
}

/// Match [`crate::user`] Prometheus naming for Rhai user metrics.
fn sanitize_user_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// Map a built-in Prometheus family name to its dataplane category
/// (design §12). Used by the emit filter in [`crate::export`].
pub fn builtin_metric_category(family_name: &str) -> Option<MetricCategory> {
    match family_name {
        "conduit_queries_total"
        | "conduit_responses_total"
        | "conduit_responses_truncated_total"
        | "conduit_queries_dropped_total"
        | "conduit_acl_decisions_total"
        | "conduit_queries_by_pool_total" => Some(MetricCategory::Volume),

        "conduit_parse_rejected_total"
        | "conduit_forward_errors_total"
        | "conduit_script_errors_total"
        | "conduit_retries_total"
        | "conduit_slot_pool_exhausted_total" => Some(MetricCategory::Failures),

        "conduit_lookup_provider_outcomes_total" | "conduit_cache_lookups_total" => {
            Some(MetricCategory::Lookup)
        }

        "conduit_phase_duration_seconds"
        | "conduit_forward_attempts_total"
        | "conduit_forward_duration_seconds"
        | "conduit_lookup_duration_seconds"
        | "conduit_cache_lookup_duration_seconds"
        | "conduit_response_duration_seconds" => Some(MetricCategory::Timing),

        "conduit_cache_fills_total"
        | "conduit_cache_singleflight_coalesced_total"
        | "conduit_cache_evictions_total"
        | "conduit_cache_entries" => Some(MetricCategory::CacheDetail),

        "conduit_forward_outstanding" => Some(MetricCategory::ForwardDetail),

        "conduit_backend_health_observed"
        | "conduit_backend_health_applied"
        | "conduit_backend_health_probe_automatic"
        | "conduit_backend_health_effective_weight"
        | "conduit_backend_health_latency_ewma_ms"
        | "conduit_backend_health_transitions_total"
        | "conduit_pool_backends_active"
        | "conduit_probe_results_total" => Some(MetricCategory::Health),

        "conduit_slots_in_use" | "conduit_slots_capacity" => Some(MetricCategory::Runtime),

        "conduit_pool_backends_configured"
        | "conduit_listener_info"
        | "conduit_listener_ingress_threads"
        | "conduit_listener_rcvbuf_bytes"
        | "conduit_backend_info"
        | "conduit_backend_weight" => Some(MetricCategory::Topology),

        "conduit_process_resident_bytes"
        | "conduit_process_open_fds"
        | "conduit_process_max_fds"
        | "conduit_process_threads"
        | "conduit_process_cpu_seconds_total" => Some(MetricCategory::Process),

        "conduit_build_info"
        | "conduit_start_time_seconds"
        | "conduit_uptime_seconds"
        | "conduit_config_generation" => Some(MetricCategory::Meta),

        _ => None,
    }
}

/// Result of resolving a [`MetricsConfig`] into a plan: the compiled plan plus
/// any non-fatal warnings (deprecation notices, disabled-with-sibling-keys,
/// exclude-failures blind spot).
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResolution {
    pub plan: CompiledMetricsPlan,
    pub warnings: Vec<String>,
}

/// Deprecation notice text for the `profile` alias (design §Migration Plan).
pub const PROFILE_DEPRECATION_WARNING: &str =
    "metrics.profile is deprecated; use metrics.base instead (profile is retained as an alias through the 1.x line, removal is scheduled for 2.x)";

/// Deprecation notice for `user_metrics[].export` (design §Decisions 11).
pub const USER_METRIC_EXPORT_DEPRECATION_WARNING: &str =
    "metrics.user_metrics[].export is deprecated; use collect/emit instead (export is retained as an alias through the 1.x line, removal is scheduled for 2.x)";

/// Resolve `metrics:` config into a [`CompiledMetricsPlan`] (design
/// §Resolution order). Pure function — logging/dedup of the one-time
/// `profile` deprecation notice happens at the call site.
///
/// # Errors
/// Returns validation error strings (never panics) on: unknown category
/// names, empty resolved category set while enabled, unknown granularity
/// family/dimension, unknown `base`/`profile` value, or `collect: false,
/// emit: true`.
pub fn resolve_metrics_plan(m: Option<&MetricsConfig>) -> Result<PlanResolution, Vec<String>> {
    let mut warnings = Vec::new();
    let Some(m) = m else {
        return Ok(PlanResolution {
            plan: CompiledMetricsPlan::disabled(),
            warnings,
        });
    };

    let profile_set = !m.profile.is_empty();
    let mut base_str = m.base.clone();
    let mut enabled = m.enabled.unwrap_or(false);

    if !enabled {
        let mut ignored = Vec::new();
        if !base_str.is_empty() {
            ignored.push("base");
        }
        if profile_set {
            ignored.push("profile");
        }
        if m.categories
            .as_ref()
            .map(|c| !c.include.is_empty() || !c.exclude.is_empty())
            .unwrap_or(false)
        {
            ignored.push("categories");
        }
        if !m.collection.is_empty() {
            ignored.push("collection");
        }
        if m.granularity
            .as_ref()
            .map(|g| !g.default.is_empty() || !g.overrides.is_empty())
            .unwrap_or(false)
        {
            ignored.push("granularity");
        }
        if m.event_export.is_some() {
            ignored.push("event_export");
        }
        if !ignored.is_empty() {
            warnings.push(format!(
                "metrics.enabled is false; ignoring metrics.{{{}}}",
                ignored.join(", ")
            ));
        }
        return Ok(PlanResolution {
            plan: CompiledMetricsPlan::disabled(),
            warnings,
        });
    }

    let mut errors = Vec::new();

    if profile_set {
        match m.profile.as_str() {
            "minimal" | "full" | "off" => {
                warnings.push(PROFILE_DEPRECATION_WARNING.to_string());
                match m.profile.as_str() {
                    "minimal" => {
                        if base_str.is_empty() {
                            base_str = "minimal".into();
                        }
                    }
                    "full" => {
                        if base_str.is_empty() {
                            base_str = "standard".into();
                        }
                    }
                    "off" => {
                        warnings.push(
                            "metrics.profile 'off' is deprecated; treating as metrics.enabled: false"
                                .into(),
                        );
                        enabled = false;
                    }
                    _ => unreachable!(),
                }
            }
            other => errors.push(format!(
                "metrics.profile '{other}' must be minimal, full, or off"
            )),
        }
    }

    if !enabled {
        if !errors.is_empty() {
            return Err(errors);
        }
        return Ok(PlanResolution {
            plan: CompiledMetricsPlan::disabled(),
            warnings,
        });
    }

    if base_str.is_empty() {
        base_str = "standard".into();
    }
    let base = match MetricsBase::from_str_opt(&base_str) {
        Some(b) => b,
        None => {
            errors.push(format!(
                "metrics.base '{base_str}' must be none, minimal, or standard"
            ));
            MetricsBase::Standard
        }
    };

    let mut categories = expand_base(base);

    if let Some(c) = &m.categories {
        let mut included = BTreeSet::new();
        for name in &c.include {
            match MetricCategory::from_str_opt(name) {
                Some(cat) => {
                    categories.insert(cat);
                    included.insert(cat);
                }
                None => errors.push(format!(
                    "metrics.categories.include '{name}' is not a known category"
                )),
            }
        }
        let mut excluded = BTreeSet::new();
        for name in &c.exclude {
            match MetricCategory::from_str_opt(name) {
                Some(cat) => {
                    excluded.insert(cat);
                }
                None => errors.push(format!(
                    "metrics.categories.exclude '{name}' is not a known category"
                )),
            }
        }
        for cat in &excluded {
            if included.contains(cat) {
                warnings.push(format!(
                    "metrics.categories: '{}' appears in both include and exclude; \
                     that is contradictory, so exclude wins and '{}' is not in the active set",
                    cat.as_str(),
                    cat.as_str()
                ));
            }
            categories.remove(cat);
        }
        if excluded.contains(&MetricCategory::Failures) {
            warnings.push(
                "metrics.categories.exclude contains 'failures'; excluding failure metrics creates an alerting blind spot"
                    .into(),
            );
        }
    }

    if categories.is_empty() {
        errors.push("metrics is enabled but the resolved category set is empty".into());
    }

    let mut granularity_default = default_granularity_for_base(base);
    let mut granularity_overrides = BTreeMap::new();
    let mut responses_rcode_override: Option<ResponsesRcodeBucketing> = None;
    // Families whose dimension list was explicitly replaced (including `[]`).
    let mut dimension_override_families = BTreeSet::new();
    if let Some(g) = &m.granularity {
        if !g.default.is_empty() {
            match Granularity::from_str_opt(&g.default) {
                Some(gr) => granularity_default = gr,
                None => errors.push(format!(
                    "metrics.granularity.default '{}' must be coarse, balanced, or fine",
                    g.default
                )),
            }
        }
        for (family, list) in &g.overrides {
            match family_allowed_dimensions(family) {
                Some(allowed) => {
                    if list.dimensions_set {
                        for dim in &list.dimensions {
                            if !allowed.contains(&dim.as_str()) {
                                errors.push(format!(
                                    "metrics.granularity.{family} dimension '{dim}' is not valid for family '{family}'"
                                ));
                            }
                        }
                        let normalized = normalize_dimensions(allowed, &list.dimensions);
                        granularity_overrides.insert(family.clone(), normalized);
                        dimension_override_families.insert(family.clone());
                    }
                    if !list.rcode.is_empty() {
                        if family != "responses" {
                            errors.push(format!(
                                "metrics.granularity.{family}.rcode is only valid under the responses family"
                            ));
                        } else {
                            match ResponsesRcodeBucketing::from_str_opt(&list.rcode) {
                                Some(mode) => responses_rcode_override = Some(mode),
                                None => errors.push(format!(
                                    "metrics.granularity.responses.rcode '{}' must be coarse or iana",
                                    list.rcode
                                )),
                            }
                        }
                    }
                }
                None => errors.push(format!("metrics.granularity has unknown family '{family}'")),
            }
        }
    }

    let responses_rcode =
        responses_rcode_override.unwrap_or_else(|| default_responses_rcode(granularity_default));

    let mut family_dimensions = BTreeMap::new();
    for family in GRANULARITY_FAMILIES {
        let dims = if dimension_override_families.contains(*family) {
            granularity_overrides
                .get(*family)
                .cloned()
                .unwrap_or_default()
        } else {
            preset_family_dimensions(family, granularity_default)
                .unwrap_or(&[])
                .iter()
                .map(|s| s.to_string())
                .collect()
        };
        family_dimensions.insert((*family).to_string(), dims);
    }

    let mut collect = BTreeMap::new();
    let mut emit = BTreeMap::new();
    for (name, ce) in &m.collection {
        let Some(cat) = MetricCategory::from_str_opt(name) else {
            errors.push(format!("metrics.collection has unknown category '{name}'"));
            continue;
        };
        let c = ce.collect.unwrap_or(true);
        let e = ce.emit.unwrap_or(true);
        if !c && e {
            errors.push(format!(
                "metrics.collection.{name}: collect: false with emit: true is invalid"
            ));
        }
        collect.insert(cat, c);
        emit.insert(cat, e);
    }
    for cat in &categories {
        collect.entry(*cat).or_insert(true);
        emit.entry(*cat).or_insert(true);
    }

    let (event_export_collect, event_export_emit) = if let Some(ee) = &m.event_export {
        let c = ee.collect.unwrap_or(true);
        let e = ee.emit.unwrap_or(true);
        if !c && e {
            errors.push("metrics.event_export: collect: false with emit: true is invalid".into());
        }
        (c, e)
    } else {
        (true, true)
    };

    // Resolve per-user-metric collect/emit (explicit keys or deprecated `export`).
    let standard_tier = granularity_default == Granularity::Fine;
    let mut user_metrics = BTreeMap::new();
    let mut seen_user = BTreeSet::new();
    let mut export_alias_used = false;
    for (i, u) in m.user_metrics.iter().enumerate() {
        if u.name.is_empty() {
            errors.push(format!("metrics.user_metrics[{i}].name must not be empty"));
            continue;
        }
        if !seen_user.insert(u.name.clone()) {
            errors.push(format!(
                "metrics.user_metrics[{i}]: duplicate name '{}'",
                u.name
            ));
            continue;
        }
        let explicit_collect = u.collect;
        let explicit_emit = u.emit;
        let has_explicit = explicit_collect.is_some() || explicit_emit.is_some();
        let has_export = !u.export.is_empty();

        if has_export {
            export_alias_used = true;
            if u.export != "minimal" && u.export != "full" {
                errors.push(format!(
                    "metrics.user_metrics[{i}].export must be 'minimal' or 'full'"
                ));
            }
        }

        let (collect, emit) = if has_explicit {
            let c = explicit_collect.unwrap_or(true);
            let e = explicit_emit.unwrap_or(true);
            if !c && e {
                errors.push(format!(
                    "metrics.user_metrics[{i}] ({}): collect: false with emit: true is invalid",
                    u.name
                ));
            }
            (c, e)
        } else if has_export {
            match u.export.as_str() {
                "minimal" => (true, true),
                "full" | "" => (standard_tier, standard_tier),
                _ => (standard_tier, standard_tier), // error already recorded
            }
        } else {
            // No override keys: same as deprecated export:full default.
            (standard_tier, standard_tier)
        };

        user_metrics.insert(
            u.name.clone(),
            UserMetricMode {
                collect,
                emit,
                help: u.help.clone(),
            },
        );
    }
    if export_alias_used {
        warnings.push(USER_METRIC_EXPORT_DEPRECATION_WARNING.to_string());
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(PlanResolution {
        plan: CompiledMetricsPlan {
            enabled: true,
            base,
            categories,
            collect,
            emit,
            granularity_default,
            granularity_overrides,
            family_dimensions,
            responses_rcode,
            event_export_collect,
            event_export_emit,
            user_metrics,
        },
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::{
        MetricsCategories, MetricsCollectEmit, MetricsConfig, MetricsDimensionList,
        MetricsEventExport, MetricsGranularity,
    };

    fn base_config() -> MetricsConfig {
        MetricsConfig {
            enabled: Some(true),
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: String::new(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        }
    }

    #[test]
    fn disabled_metrics_produce_disabled_plan_with_no_warnings() {
        let res = resolve_metrics_plan(None).unwrap();
        assert!(!res.plan.enabled);
        assert!(res.warnings.is_empty());
    }

    #[test]
    fn default_enabled_with_no_base_resolves_to_standard() {
        let cfg = base_config();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.base, MetricsBase::Standard);
        assert_eq!(res.plan.granularity_default, Granularity::Fine);
        for cat in STANDARD_CATEGORIES {
            assert!(res.plan.categories.contains(cat), "missing {cat:?}");
        }
        assert!(res.warnings.is_empty());
    }

    #[test]
    fn minimal_base_expands_to_volume_and_failures_and_health() {
        let mut cfg = base_config();
        cfg.base = "minimal".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.granularity_default, Granularity::Coarse);
        assert!(res.plan.categories.contains(&MetricCategory::Volume));
        assert!(res.plan.categories.contains(&MetricCategory::Failures));
        assert!(
            res.plan.categories.contains(&MetricCategory::Health),
            "minimal must include health (intentional 1.x expansion)"
        );
        assert!(!res.plan.categories.contains(&MetricCategory::Timing));
    }

    #[test]
    fn none_base_with_no_include_errors_empty_category_set() {
        let mut cfg = base_config();
        cfg.base = "none".into();
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("empty")), "{err:?}");
    }

    #[test]
    fn none_base_with_include_resolves() {
        let mut cfg = base_config();
        cfg.base = "none".into();
        cfg.categories = Some(MetricsCategories {
            include: vec!["volume".into()],
            exclude: vec![],
            include_set: true,
            exclude_set: true,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.categories.len(), 1);
        assert!(res.plan.categories.contains(&MetricCategory::Volume));
    }

    #[test]
    fn include_and_exclude_together() {
        let mut cfg = base_config();
        cfg.base = "minimal".into();
        cfg.categories = Some(MetricsCategories {
            include: vec!["timing".into()],
            exclude: vec!["failures".into()],
            include_set: true,
            exclude_set: true,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.categories.contains(&MetricCategory::Volume));
        assert!(res.plan.categories.contains(&MetricCategory::Timing));
        assert!(!res.plan.categories.contains(&MetricCategory::Failures));
        assert!(res
            .warnings
            .iter()
            .any(|w| w.contains("failures") && w.contains("blind spot")));
        assert!(
            !res.warnings
                .iter()
                .any(|w| w.contains("both include and exclude")),
            "distinct include/exclude names must not emit an overlap warning: {:?}",
            res.warnings
        );
    }

    #[test]
    fn category_in_both_include_and_exclude_warns_and_exclude_wins() {
        let mut cfg = base_config();
        cfg.base = "none".into();
        cfg.categories = Some(MetricsCategories {
            include: vec!["timing".into(), "volume".into()],
            exclude: vec!["timing".into()],
            include_set: true,
            exclude_set: true,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.categories.contains(&MetricCategory::Volume));
        assert!(!res.plan.categories.contains(&MetricCategory::Timing));
        assert!(
            res.warnings.iter().any(|w| {
                w.contains("timing")
                    && w.contains("both include and exclude")
                    && w.contains("exclude wins")
            }),
            "{:?}",
            res.warnings
        );
    }

    #[test]
    fn unknown_category_name_errors() {
        let mut cfg = base_config();
        cfg.categories = Some(MetricsCategories {
            include: vec!["bogus".into()],
            exclude: vec![],
            include_set: true,
            exclude_set: true,
        });
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("bogus")), "{err:?}");
    }

    #[test]
    fn invalid_collect_false_emit_true_errors() {
        let mut cfg = base_config();
        cfg.collection.insert(
            "timing".into(),
            MetricsCollectEmit {
                collect: Some(false),
                emit: Some(true),
            },
        );
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("collect: false") && e.contains("emit: true")),
            "{err:?}"
        );
    }

    #[test]
    fn collect_true_emit_false_is_valid_and_host_only() {
        let mut cfg = base_config();
        cfg.collection.insert(
            "timing".into(),
            MetricsCollectEmit {
                collect: Some(true),
                emit: Some(false),
            },
        );
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.collect_for(MetricCategory::Timing));
        assert!(!res.plan.emit_for(MetricCategory::Timing));
    }

    #[test]
    fn event_export_invalid_combo_errors() {
        let mut cfg = base_config();
        cfg.event_export = Some(MetricsEventExport {
            collect: Some(false),
            emit: Some(true),
        });
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("event_export")), "{err:?}");
    }

    #[test]
    fn event_export_defaults_true_when_omitted() {
        let cfg = base_config();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.event_export_collect);
        assert!(res.plan.event_export_emit);
    }

    fn dims(dimensions: Vec<&str>) -> MetricsDimensionList {
        MetricsDimensionList {
            dimensions: dimensions.into_iter().map(str::to_string).collect(),
            rcode: String::new(),
            dimensions_set: true,
        }
    }

    fn responses_rcode_only(rcode: &str) -> MetricsDimensionList {
        MetricsDimensionList {
            dimensions: vec![],
            rcode: rcode.into(),
            dimensions_set: false,
        }
    }

    #[test]
    fn unknown_granularity_family_errors() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("bogus_family".to_string(), dims(vec!["pool"]));
        cfg.granularity = Some(MetricsGranularity {
            default: String::new(),
            overrides,
        });
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("bogus_family")), "{err:?}");
    }

    #[test]
    fn invalid_dimension_for_family_errors() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("timing".to_string(), dims(vec!["qname"]));
        cfg.granularity = Some(MetricsGranularity {
            default: String::new(),
            overrides,
        });
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("qname")), "{err:?}");
    }

    #[test]
    fn granularity_override_normalizes_dimension_order() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("timing".to_string(), dims(vec!["backend", "pool"]));
        cfg.granularity = Some(MetricsGranularity {
            default: String::new(),
            overrides,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan.granularity_overrides.get("timing").unwrap(),
            &vec!["pool".to_string(), "backend".to_string()]
        );
        assert_eq!(
            res.plan.dimensions_for("timing"),
            &["pool".to_string(), "backend".to_string()]
        );
    }

    #[test]
    fn backend_without_pool_allowed() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("forward_failures".to_string(), dims(vec!["backend"]));
        cfg.granularity = Some(MetricsGranularity {
            default: String::new(),
            overrides,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan
                .granularity_overrides
                .get("forward_failures")
                .unwrap(),
            &vec!["backend".to_string()]
        );
        assert_eq!(
            res.plan.dimensions_for("forward_failures"),
            &["backend".to_string()]
        );
    }

    #[test]
    fn balanced_default_expands_timing_to_pool_only() {
        let mut cfg = base_config();
        cfg.granularity = Some(MetricsGranularity {
            default: "balanced".into(),
            overrides: Default::default(),
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.granularity_default, Granularity::Balanced);
        assert_eq!(res.plan.dimensions_for("timing"), &["pool".to_string()]);
        assert_eq!(
            res.plan.dimensions_for("volume"),
            &[
                "listener".to_string(),
                "protocol".to_string(),
                "qtype".to_string()
            ]
        );
        assert_eq!(res.plan.responses_rcode, ResponsesRcodeBucketing::Coarse);
    }

    #[test]
    fn timing_override_replaces_balanced_preset() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("timing".to_string(), dims(vec!["pool", "backend"]));
        cfg.granularity = Some(MetricsGranularity {
            default: "balanced".into(),
            overrides,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan.dimensions_for("timing"),
            &["pool".to_string(), "backend".to_string()]
        );
    }

    #[test]
    fn empty_timing_dimensions_aggregate() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("timing".to_string(), dims(vec![]));
        cfg.granularity = Some(MetricsGranularity {
            default: "fine".into(),
            overrides,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.dimensions_for("timing").is_empty());
        // Other families still use fine preset.
        assert_eq!(
            res.plan.dimensions_for("volume"),
            &[
                "listener".to_string(),
                "protocol".to_string(),
                "qtype".to_string(),
                "qclass".to_string(),
                "ip_family".to_string()
            ]
        );
    }

    #[test]
    fn responses_rcode_only_override_keeps_preset_dimensions() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("responses".to_string(), responses_rcode_only("coarse"));
        cfg.granularity = Some(MetricsGranularity {
            default: "fine".into(),
            overrides,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.responses_rcode, ResponsesRcodeBucketing::Coarse);
        assert_eq!(
            res.plan.dimensions_for("responses"),
            &[
                "listener".to_string(),
                "protocol".to_string(),
                "rcode".to_string(),
                "ip_family".to_string(),
                "answer_source".to_string()
            ]
        );
    }

    #[test]
    fn default_standard_plan_matches_shipped_full_schemas() {
        let cfg = base_config();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.base, MetricsBase::Standard);
        assert_eq!(res.plan.granularity_default, Granularity::Fine);
        assert_eq!(res.plan.responses_rcode, ResponsesRcodeBucketing::Iana);
        assert_eq!(
            res.plan.dimensions_for("volume"),
            &[
                "listener".to_string(),
                "protocol".to_string(),
                "qtype".to_string(),
                "qclass".to_string(),
                "ip_family".to_string()
            ]
        );
        assert_eq!(
            res.plan.dimensions_for("responses"),
            &[
                "listener".to_string(),
                "protocol".to_string(),
                "rcode".to_string(),
                "ip_family".to_string(),
                "answer_source".to_string()
            ]
        );
        assert_eq!(
            res.plan.dimensions_for("timing"),
            &["pool".to_string(), "backend".to_string()]
        );
        assert_eq!(
            res.plan.dimensions_for("forward_failures"),
            &[
                "pool".to_string(),
                "backend".to_string(),
                "reason".to_string()
            ]
        );
        assert_eq!(
            res.plan.dimensions_for("acl"),
            &[
                "tier".to_string(),
                "action".to_string(),
                "listener".to_string(),
                "ip_family".to_string()
            ]
        );
    }

    #[test]
    fn profile_full_alias_matches_shipped_full_schemas() {
        let mut cfg = base_config();
        cfg.profile = "full".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan.dimensions_for("timing"),
            &["pool".to_string(), "backend".to_string()]
        );
        assert_eq!(res.plan.responses_rcode, ResponsesRcodeBucketing::Iana);
    }

    #[test]
    fn minimal_base_uses_coarse_presets() {
        let mut cfg = base_config();
        cfg.base = "minimal".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan.dimensions_for("volume"),
            &["listener".to_string(), "protocol".to_string()]
        );
        assert!(res.plan.dimensions_for("timing").is_empty());
        assert_eq!(res.plan.responses_rcode, ResponsesRcodeBucketing::Coarse);
    }

    #[test]
    fn disabled_with_sibling_keys_warns_and_ignores() {
        let mut cfg = base_config();
        cfg.enabled = Some(false);
        cfg.base = "standard".into();
        cfg.categories = Some(MetricsCategories {
            include: vec![],
            exclude: vec!["process".into()],
            include_set: true,
            exclude_set: true,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(!res.plan.enabled);
        assert!(res.plan.categories.is_empty());
        assert!(
            res.warnings.iter().any(|w| w.contains("ignoring")),
            "{:?}",
            res.warnings
        );
    }

    #[test]
    fn profile_minimal_alias_maps_to_base_minimal() {
        let mut cfg = base_config();
        cfg.profile = "minimal".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.base, MetricsBase::Minimal);
        assert!(res
            .warnings
            .contains(&PROFILE_DEPRECATION_WARNING.to_string()));
    }

    #[test]
    fn profile_full_alias_maps_to_base_standard_with_warning() {
        let mut cfg = base_config();
        cfg.profile = "full".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.base, MetricsBase::Standard);
        assert_eq!(res.plan.granularity_default, Granularity::Fine);
        assert!(res
            .warnings
            .contains(&PROFILE_DEPRECATION_WARNING.to_string()));
    }

    #[test]
    fn profile_off_with_enabled_true_normalizes_to_disabled() {
        let mut cfg = base_config();
        cfg.profile = "off".into();
        cfg.enabled = Some(true);
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(!res.plan.enabled);
        assert!(res.warnings.iter().any(|w| w.contains("enabled: false")));
    }

    #[test]
    fn base_wins_when_both_base_and_profile_set() {
        let mut cfg = base_config();
        cfg.profile = "minimal".into();
        cfg.base = "standard".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(res.plan.base, MetricsBase::Standard);
    }

    #[test]
    fn invalid_profile_value_errors() {
        let mut cfg = base_config();
        cfg.profile = "bogus".into();
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("bogus")), "{err:?}");
    }

    #[test]
    fn invalid_base_value_errors() {
        let mut cfg = base_config();
        cfg.base = "bogus".into();
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("bogus")), "{err:?}");
    }

    #[test]
    fn same_metric_name_share_across_plans() {
        // Compat/parity: `conduit_queries_total` should be reachable under both
        // minimal and standard plans once volume is resolved (design §"Same
        // metric names across plans").
        let mut minimal = base_config();
        minimal.base = "minimal".into();
        let mut standard = base_config();
        standard.base = "standard".into();
        let r1 = resolve_metrics_plan(Some(&minimal)).unwrap();
        let r2 = resolve_metrics_plan(Some(&standard)).unwrap();
        assert!(r1.plan.categories.contains(&MetricCategory::Volume));
        assert!(r2.plan.categories.contains(&MetricCategory::Volume));
    }

    #[test]
    fn user_metric_help_carried_in_plan() {
        use conduit_proto::config::UserMetricExportConfig;
        let mut cfg = base_config();
        cfg.user_metrics = vec![UserMetricExportConfig {
            name: "block_hits".into(),
            export: String::new(),
            collect: Some(true),
            emit: Some(true),
            help: "Policy block hits by category".into(),
        }];
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan.user_help_for("block_hits"),
            "Policy block hits by category"
        );
        let helps = res.plan.user_helps();
        assert_eq!(
            helps.get("block_hits").map(String::as_str),
            Some("Policy block hits by category")
        );
        assert_eq!(res.plan.user_help_for("unknown"), "");
    }

    #[test]
    fn user_metric_collect_only_explicit() {
        use conduit_proto::config::UserMetricExportConfig;
        let mut cfg = base_config();
        cfg.user_metrics = vec![UserMetricExportConfig {
            name: "block_hits".into(),
            export: String::new(),
            collect: Some(true),
            emit: Some(false),
            help: String::new(),
        }];
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.user_collect_for("block_hits"));
        assert!(!res.plan.user_emit_for("block_hits"));
        assert!(!res.plan.emits_family("conduit_user_block_hits"));
        assert!(res.warnings.is_empty());
    }

    #[test]
    fn user_metric_export_minimal_alias_with_deprecation() {
        use conduit_proto::config::UserMetricExportConfig;
        let mut cfg = base_config();
        cfg.base = "minimal".into();
        cfg.user_metrics = vec![UserMetricExportConfig {
            name: "block_hits".into(),
            export: "minimal".into(),
            collect: None,
            emit: None,
            help: String::new(),
        }];
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.user_collect_for("block_hits"));
        assert!(res.plan.user_emit_for("block_hits"));
        assert!(res
            .warnings
            .contains(&USER_METRIC_EXPORT_DEPRECATION_WARNING.to_string()));
    }

    #[test]
    fn user_metric_export_full_default_off_on_minimal() {
        let mut cfg = base_config();
        cfg.base = "minimal".into();
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(!res.plan.user_collect_for("block_hits"));
        assert!(!res.plan.user_emit_for("block_hits"));
    }

    #[test]
    fn user_metric_invalid_collect_emit_combo() {
        use conduit_proto::config::UserMetricExportConfig;
        let mut cfg = base_config();
        cfg.user_metrics = vec![UserMetricExportConfig {
            name: "block_hits".into(),
            export: String::new(),
            collect: Some(false),
            emit: Some(true),
            help: String::new(),
        }];
        let err = resolve_metrics_plan(Some(&cfg)).unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("collect: false") && e.contains("emit: true")),
            "{err:?}"
        );
    }

    #[test]
    fn emit_mask_filters_builtin_category() {
        let mut cfg = base_config();
        cfg.collection.insert(
            "timing".into(),
            MetricsCollectEmit {
                collect: Some(true),
                emit: Some(false),
            },
        );
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.collect_for(MetricCategory::Timing));
        assert!(!res.plan.emit_for(MetricCategory::Timing));
        assert!(!res.plan.emits_family("conduit_forward_attempts_total"));
        assert!(res.plan.emits_family("conduit_queries_total"));
    }

    #[test]
    fn meta_families_map_to_meta_category() {
        for name in [
            "conduit_build_info",
            "conduit_start_time_seconds",
            "conduit_uptime_seconds",
            "conduit_config_generation",
        ] {
            assert_eq!(
                builtin_metric_category(name),
                Some(MetricCategory::Meta),
                "{name}"
            );
        }
    }

    #[test]
    fn process_families_map_to_process_category() {
        for name in [
            "conduit_process_resident_bytes",
            "conduit_process_open_fds",
            "conduit_process_max_fds",
            "conduit_process_threads",
            "conduit_process_cpu_seconds_total",
        ] {
            assert_eq!(
                builtin_metric_category(name),
                Some(MetricCategory::Process),
                "{name}"
            );
        }
        let mut cfg = base_config();
        cfg.collection.insert(
            "process".into(),
            MetricsCollectEmit {
                collect: Some(true),
                emit: Some(false),
            },
        );
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(!res.plan.emits_family("conduit_process_max_fds"));
        assert!(!res.plan.emits_family("conduit_process_cpu_seconds_total"));
        assert!(res.plan.emits_family("conduit_queries_total"));
    }

    #[test]
    fn event_export_emit_false_filters_events_families() {
        let mut cfg = base_config();
        cfg.event_export = Some(MetricsEventExport {
            collect: Some(true),
            emit: Some(false),
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.event_export_collect);
        assert!(!res.plan.event_export_emit);
        assert!(!res.plan.emits_family("conduit_events_enqueued_query_total"));
    }
}
