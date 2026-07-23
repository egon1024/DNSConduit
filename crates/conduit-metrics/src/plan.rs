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

/// Maintainer table: closed dimension vocabulary per family, in canonical
/// order (used to normalize operator-provided lists at compile time).
///
/// G1 scope: validated here; **not yet applied** to `BuiltinRegistry`
/// registration (per-family granularity wiring is Phase C / gate G3).
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
    /// Validated, normalized per-family dimension overrides. Not yet applied
    /// to registration (G3); reserved for the granularity phase.
    pub granularity_overrides: BTreeMap<String, Vec<String>>,
    pub event_export_collect: bool,
    pub event_export_emit: bool,
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
            event_export_collect: false,
            event_export_emit: false,
        }
    }

    /// Host/hot-path collect mask for `cat` (design §Decisions 4).
    pub fn collect_for(&self, cat: MetricCategory) -> bool {
        self.enabled && self.categories.contains(&cat) && *self.collect.get(&cat).unwrap_or(&true)
    }

    /// Prometheus/OTLP emit mask for `cat` (design §Decisions 4).
    pub fn emit_for(&self, cat: MetricCategory) -> bool {
        self.enabled && self.categories.contains(&cat) && *self.emit.get(&cat).unwrap_or(&true)
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
    let mut enabled = m.enabled;

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
        for name in &c.include {
            match MetricCategory::from_str_opt(name) {
                Some(cat) => {
                    categories.insert(cat);
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
                    for dim in &list.dimensions {
                        if !allowed.contains(&dim.as_str()) {
                            errors.push(format!(
                                "metrics.granularity.{family} dimension '{dim}' is not valid for family '{family}'"
                            ));
                        }
                    }
                    let normalized: Vec<String> = allowed
                        .iter()
                        .filter(|dim| list.dimensions.iter().any(|d| d == *dim))
                        .map(|dim| dim.to_string())
                        .collect();
                    granularity_overrides.insert(family.clone(), normalized);
                }
                None => errors.push(format!("metrics.granularity has unknown family '{family}'")),
            }
        }
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
            event_export_collect,
            event_export_emit,
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
            enabled: true,
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
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert!(res.plan.categories.contains(&MetricCategory::Volume));
        assert!(res.plan.categories.contains(&MetricCategory::Timing));
        assert!(!res.plan.categories.contains(&MetricCategory::Failures));
        assert!(res
            .warnings
            .iter()
            .any(|w| w.contains("failures") && w.contains("blind spot")));
    }

    #[test]
    fn unknown_category_name_errors() {
        let mut cfg = base_config();
        cfg.categories = Some(MetricsCategories {
            include: vec!["bogus".into()],
            exclude: vec![],
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

    #[test]
    fn unknown_granularity_family_errors() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "bogus_family".to_string(),
            MetricsDimensionList {
                dimensions: vec!["pool".into()],
            },
        );
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
        overrides.insert(
            "timing".to_string(),
            MetricsDimensionList {
                dimensions: vec!["qname".into()],
            },
        );
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
        overrides.insert(
            "timing".to_string(),
            MetricsDimensionList {
                dimensions: vec!["backend".into(), "pool".into()],
            },
        );
        cfg.granularity = Some(MetricsGranularity {
            default: String::new(),
            overrides,
        });
        let res = resolve_metrics_plan(Some(&cfg)).unwrap();
        assert_eq!(
            res.plan.granularity_overrides.get("timing").unwrap(),
            &vec!["pool".to_string(), "backend".to_string()]
        );
    }

    #[test]
    fn backend_without_pool_allowed() {
        let mut cfg = base_config();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "forward_failures".to_string(),
            MetricsDimensionList {
                dimensions: vec!["backend".into()],
            },
        );
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
    }

    #[test]
    fn disabled_with_sibling_keys_warns_and_ignores() {
        let mut cfg = base_config();
        cfg.enabled = false;
        cfg.base = "standard".into();
        cfg.categories = Some(MetricsCategories {
            include: vec![],
            exclude: vec!["process".into()],
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
        cfg.enabled = true;
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
}
