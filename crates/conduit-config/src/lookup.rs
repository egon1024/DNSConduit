//! Compiled lookup profiles and named cache instances (pluggable-lookup-system).

use conduit_proto::config::{
    CacheInstance, CacheMemoryConfig, CacheNegativeConfig, CacheOnHitConfig,
    CacheTruncatedUdpConfig, Config, LookupProfile, LookupProvider,
};
use std::collections::{HashMap, HashSet};

pub const DEFAULT_LOOKUP_PROFILE: &str = "default";
pub const DEFAULT_SERVFAIL_TTL_SECS: u32 = 10;
pub const DEFAULT_TRUNCATED_UDP_TTL_SECS: u32 = 60;
pub const DEFAULT_ON_HIT_RESPONSE_RULES: &str = "skip";
pub const DEFAULT_EVICTION_MODE: &str = "passive";
pub const DEFAULT_MEMORY_SHARD_COUNT: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheBackendType {
    Memory,
    /// Reserved — not implemented in v1.
    Lmdb,
    /// Reserved — not implemented in v1.
    EbpfMap,
}

impl CacheBackendType {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "memory" => Ok(Self::Memory),
            "lmdb" => Ok(Self::Lmdb),
            "ebpf_map" => Ok(Self::EbpfMap),
            other => Err(format!(
                "cache type '{other}' must be memory (lmdb and ebpf_map are reserved)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Lmdb => "lmdb",
            Self::EbpfMap => "ebpf_map",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnHitResponseRules {
    Skip,
    Run,
}

impl OnHitResponseRules {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "skip" => Ok(Self::Skip),
            "" | "run" => Ok(Self::Run),
            other => Err(format!(
                "on_hit.response_rules '{other}' must be skip or run"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Run => "run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionMode {
    Passive,
    Active,
}

impl EvictionMode {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "" | "passive" => Ok(Self::Passive),
            "active" => Ok(Self::Active),
            other => Err(format!(
                "memory.eviction '{other}' must be passive or active"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledLookupProvider {
    Cache { cache_name: String },
    Forward,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledNegativeCache {
    pub enabled: bool,
    pub nxdomain_covers_descendants: bool,
    pub servfail_ttl_secs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledMemoryCache {
    pub shard_count: u32,
    pub eviction: EvictionMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledTruncatedUdp {
    pub enabled: bool,
    pub ttl_secs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledCacheInstance {
    pub name: String,
    pub backend_type: CacheBackendType,
    pub negative_cache: CompiledNegativeCache,
    pub on_hit_response_rules: OnHitResponseRules,
    pub truncated_udp: CompiledTruncatedUdp,
    pub rotate_rrset_on_serve: bool,
    pub memory: CompiledMemoryCache,
    pub max_entries: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLookupProfile {
    pub name: String,
    pub providers: Vec<CompiledLookupProvider>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompiledLookup {
    pub enabled: bool,
    pub profiles: HashMap<String, CompiledLookupProfile>,
    pub cache_instances: HashMap<String, CompiledCacheInstance>,
    /// True when any profile lists a cache provider (off-when-disabled gate).
    pub cache_provider_configured: bool,
}

impl CompiledLookup {
    pub fn compile_from_config(cfg: &Config) -> Result<Self, String> {
        let cache_instances = compile_cache_instances(&cfg.caches)?;
        let lookup_cfg = match cfg.lookup.as_ref() {
            None => {
                let mut profiles = HashMap::new();
                profiles.insert(
                    DEFAULT_LOOKUP_PROFILE.to_string(),
                    CompiledLookupProfile {
                        name: DEFAULT_LOOKUP_PROFILE.to_string(),
                        providers: vec![CompiledLookupProvider::Forward],
                    },
                );
                return Ok(Self {
                    enabled: true,
                    profiles,
                    cache_instances,
                    cache_provider_configured: false,
                });
            }
            Some(l) => l,
        };

        if lookup_cfg.profiles.is_empty() {
            return Err(
                "lookup.profiles must contain at least one profile when lookup is configured"
                    .into(),
            );
        }

        let has_cache_provider = lookup_cfg
            .profiles
            .values()
            .any(|profile| profile.providers.iter().any(|p| p.r#type.trim() == "cache"));
        if has_cache_provider && cfg.caches.is_empty() {
            return Err("lookup profile references cache provider but caches list is empty".into());
        }

        let mut profiles = HashMap::new();
        let mut cache_provider_configured = false;

        for (name, profile) in &lookup_cfg.profiles {
            if name.is_empty() {
                return Err("lookup profile name must not be empty".into());
            }
            let compiled = compile_profile(name, profile, &cache_instances)?;
            if compiled
                .providers
                .iter()
                .any(|p| matches!(p, CompiledLookupProvider::Cache { .. }))
            {
                cache_provider_configured = true;
            }
            profiles.insert(name.clone(), compiled);
        }

        if cache_provider_configured && cfg.caches.is_empty() {
            return Err("lookup profile references cache provider but caches list is empty".into());
        }

        Ok(Self {
            enabled: true,
            profiles,
            cache_instances,
            cache_provider_configured,
        })
    }

    pub fn lookup_enabled(&self) -> bool {
        self.enabled
    }

    pub fn cache_enabled(&self) -> bool {
        self.cache_provider_configured
    }

    pub fn default_profile(&self) -> Option<&CompiledLookupProfile> {
        self.profiles.get(DEFAULT_LOOKUP_PROFILE)
    }
}

fn compile_profile(
    name: &str,
    profile: &LookupProfile,
    cache_instances: &HashMap<String, CompiledCacheInstance>,
) -> Result<CompiledLookupProfile, String> {
    if profile.providers.is_empty() {
        return Err(format!(
            "lookup.profiles.{name}.providers must contain at least one provider"
        ));
    }

    let mut providers = Vec::with_capacity(profile.providers.len());
    for (idx, provider) in profile.providers.iter().enumerate() {
        providers.push(compile_provider(name, idx, provider, cache_instances)?);
    }

    Ok(CompiledLookupProfile {
        name: name.to_string(),
        providers,
    })
}

fn compile_provider(
    profile_name: &str,
    idx: usize,
    provider: &LookupProvider,
    cache_instances: &HashMap<String, CompiledCacheInstance>,
) -> Result<CompiledLookupProvider, String> {
    let ctx = format!("lookup.profiles.{profile_name}.providers[{idx}]");
    match provider.r#type.trim() {
        "forward" => Ok(CompiledLookupProvider::Forward),
        "cache" => {
            let cache_name = provider
                .cache
                .as_ref()
                .filter(|n| !n.is_empty())
                .ok_or_else(|| format!("{ctx}: cache provider requires cache instance name"))?;
            if !cache_instances.contains_key(cache_name) {
                return Err(format!("{ctx}: unknown cache instance '{cache_name}'"));
            }
            Ok(CompiledLookupProvider::Cache {
                cache_name: cache_name.clone(),
            })
        }
        "" => Err(format!("{ctx}: provider type must not be empty")),
        other => Err(format!(
            "{ctx}: provider type '{other}' must be cache or forward"
        )),
    }
}

fn compile_cache_instances(
    instances: &[CacheInstance],
) -> Result<HashMap<String, CompiledCacheInstance>, String> {
    let mut names = HashSet::new();
    let mut compiled = HashMap::new();

    for (idx, instance) in instances.iter().enumerate() {
        let ctx = format!("caches[{idx}]");
        if instance.name.is_empty() {
            return Err(format!("{ctx}: name must not be empty"));
        }
        if !names.insert(instance.name.clone()) {
            return Err(format!("duplicate cache name '{}'", instance.name));
        }

        let backend_type =
            CacheBackendType::parse(&instance.r#type).map_err(|e| format!("{ctx}: {e}"))?;
        if backend_type != CacheBackendType::Memory {
            return Err(format!(
                "{ctx}: type '{}' is reserved and not implemented in v1",
                backend_type.as_str()
            ));
        }

        let negative_cache = compile_negative_cache(instance.negative_cache.as_ref())?;
        let on_hit_response_rules = compile_on_hit(instance.on_hit.as_ref(), &ctx)?;
        let memory = compile_memory(instance.memory.as_ref(), &ctx)?;
        let truncated_udp = compile_truncated_udp(instance.truncated_udp.as_ref(), &ctx)?;

        compiled.insert(
            instance.name.clone(),
            CompiledCacheInstance {
                name: instance.name.clone(),
                backend_type,
                negative_cache,
                on_hit_response_rules,
                truncated_udp,
                rotate_rrset_on_serve: instance.rotate_rrset_on_serve.unwrap_or(false),
                memory,
                max_entries: instance.max_entries.unwrap_or(0),
            },
        );
    }

    Ok(compiled)
}

fn compile_truncated_udp(
    cfg: Option<&CacheTruncatedUdpConfig>,
    ctx: &str,
) -> Result<CompiledTruncatedUdp, String> {
    match cfg {
        None => Ok(CompiledTruncatedUdp {
            enabled: false,
            ttl_secs: DEFAULT_TRUNCATED_UDP_TTL_SECS,
        }),
        Some(t) => {
            let enabled = t.enabled.unwrap_or(false);
            let ttl_secs = if enabled {
                match t.ttl_secs {
                    None | Some(0) => {
                        return Err(format!(
                            "{ctx}.truncated_udp.ttl_secs must be set and > 0 when truncated_udp.enabled is true"
                        ));
                    }
                    Some(ttl) => ttl,
                }
            } else {
                t.ttl_secs.unwrap_or(DEFAULT_TRUNCATED_UDP_TTL_SECS)
            };
            Ok(CompiledTruncatedUdp { enabled, ttl_secs })
        }
    }
}

fn compile_negative_cache(
    cfg: Option<&CacheNegativeConfig>,
) -> Result<CompiledNegativeCache, String> {
    match cfg {
        None => Ok(CompiledNegativeCache {
            enabled: true,
            nxdomain_covers_descendants: true,
            servfail_ttl_secs: DEFAULT_SERVFAIL_TTL_SECS,
        }),
        Some(n) => Ok(CompiledNegativeCache {
            enabled: n.enabled.unwrap_or(true),
            nxdomain_covers_descendants: n.nxdomain_covers_descendants.unwrap_or(true),
            servfail_ttl_secs: n.servfail_ttl_secs.unwrap_or(DEFAULT_SERVFAIL_TTL_SECS),
        }),
    }
}

fn compile_on_hit(cfg: Option<&CacheOnHitConfig>, ctx: &str) -> Result<OnHitResponseRules, String> {
    match cfg {
        None => Ok(OnHitResponseRules::Run),
        Some(o) => {
            OnHitResponseRules::parse(&o.response_rules).map_err(|e| format!("{ctx}.on_hit: {e}"))
        }
    }
}

fn compile_memory(
    cfg: Option<&CacheMemoryConfig>,
    ctx: &str,
) -> Result<CompiledMemoryCache, String> {
    match cfg {
        None => Ok(CompiledMemoryCache {
            shard_count: DEFAULT_MEMORY_SHARD_COUNT,
            eviction: EvictionMode::Passive,
        }),
        Some(m) => {
            let shard_count = m.shard_count.unwrap_or(DEFAULT_MEMORY_SHARD_COUNT);
            if shard_count == 0 {
                return Err(format!("{ctx}.memory.shard_count must be >= 1"));
            }
            let eviction =
                EvictionMode::parse(m.eviction.as_deref().unwrap_or(DEFAULT_EVICTION_MODE))
                    .map_err(|e| format!("{ctx}.memory: {e}"))?;
            Ok(CompiledMemoryCache {
                shard_count,
                eviction,
            })
        }
    }
}

pub fn validate_lookup(cfg: &Config) -> Vec<String> {
    match CompiledLookup::compile_from_config(cfg) {
        Ok(_) => Vec::new(),
        Err(e) => vec![e],
    }
}

pub fn compile_lookup_from_config(cfg: &Config) -> Result<CompiledLookup, String> {
    CompiledLookup::compile_from_config(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::load_yaml;
    use conduit_proto::config::{
        CacheInstance, CacheOnHitConfig, CacheTruncatedUdpConfig, LookupConfig, LookupProfile,
        LookupProvider,
    };

    fn minimal_with_lookup(lookup: LookupConfig, caches: Vec<CacheInstance>) -> Config {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).expect("minimal");
        cfg.lookup = Some(lookup);
        cfg.caches = caches;
        cfg
    }

    #[test]
    fn forward_only_profile_compiles() {
        let lookup = LookupConfig {
            profiles: HashMap::from([(
                "default".into(),
                LookupProfile {
                    providers: vec![LookupProvider {
                        r#type: "forward".into(),
                        cache: None,
                    }],
                },
            )]),
        };
        let compiled = CompiledLookup::compile_from_config(&minimal_with_lookup(lookup, vec![]))
            .expect("compile");
        assert!(compiled.lookup_enabled());
        assert!(!compiled.cache_enabled());
        assert_eq!(compiled.profiles["default"].providers.len(), 1);
    }

    #[test]
    fn cache_profile_requires_instance() {
        let lookup = LookupConfig {
            profiles: HashMap::from([(
                "default".into(),
                LookupProfile {
                    providers: vec![LookupProvider {
                        r#type: "cache".into(),
                        cache: Some("global".into()),
                    }],
                },
            )]),
        };
        let err =
            CompiledLookup::compile_from_config(&minimal_with_lookup(lookup, vec![])).unwrap_err();
        assert!(err.contains("caches list is empty"));
    }

    #[test]
    fn invalid_cache_ref_rejected() {
        let lookup = LookupConfig {
            profiles: HashMap::from([(
                "default".into(),
                LookupProfile {
                    providers: vec![LookupProvider {
                        r#type: "cache".into(),
                        cache: Some("missing".into()),
                    }],
                },
            )]),
        };
        let caches = vec![CacheInstance {
            name: "global".into(),
            r#type: "memory".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            key: None,
            max_entries: None,
        }];
        let err =
            CompiledLookup::compile_from_config(&minimal_with_lookup(lookup, caches)).unwrap_err();
        assert!(err.contains("unknown cache instance 'missing'"));
    }

    #[test]
    fn on_hit_defaults_to_run() {
        let caches = vec![CacheInstance {
            name: "global".into(),
            r#type: "memory".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            key: None,
            max_entries: None,
        }];
        let compiled = CompiledLookup::compile_from_config(&minimal_with_lookup(
            LookupConfig {
                profiles: HashMap::from([(
                    "default".into(),
                    LookupProfile {
                        providers: vec![LookupProvider {
                            r#type: "cache".into(),
                            cache: Some("global".into()),
                        }],
                    },
                )]),
            },
            caches,
        ))
        .unwrap();
        assert_eq!(
            compiled.cache_instances["global"].on_hit_response_rules,
            OnHitResponseRules::Run
        );
    }

    #[test]
    fn invalid_on_hit_rejected() {
        let caches = vec![CacheInstance {
            name: "global".into(),
            r#type: "memory".into(),
            negative_cache: None,
            on_hit: Some(CacheOnHitConfig {
                response_rules: "maybe".into(),
            }),
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            key: None,
            max_entries: None,
        }];
        let err = CompiledLookup::compile_from_config(&minimal_with_lookup(
            LookupConfig {
                profiles: HashMap::from([(
                    "default".into(),
                    LookupProfile {
                        providers: vec![LookupProvider {
                            r#type: "cache".into(),
                            cache: Some("global".into()),
                        }],
                    },
                )]),
            },
            caches,
        ))
        .unwrap_err();
        assert!(err.contains("on_hit.response_rules"));
    }

    #[test]
    fn truncated_udp_ttl_required_when_enabled() {
        let caches = vec![CacheInstance {
            name: "global".into(),
            r#type: "memory".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: Some(CacheTruncatedUdpConfig {
                enabled: Some(true),
                ttl_secs: None,
            }),
            rotate_rrset_on_serve: None,
            memory: None,
            key: None,
            max_entries: None,
        }];
        let err = CompiledLookup::compile_from_config(&minimal_with_lookup(
            LookupConfig {
                profiles: HashMap::from([(
                    "default".into(),
                    LookupProfile {
                        providers: vec![LookupProvider {
                            r#type: "cache".into(),
                            cache: Some("global".into()),
                        }],
                    },
                )]),
            },
            caches,
        ))
        .unwrap_err();
        assert!(err.contains("truncated_udp.ttl_secs"));
    }

    #[test]
    fn lookup_absent_synthesizes_implicit_default_forward_profile() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).expect("minimal");
        let compiled = CompiledLookup::compile_from_config(&cfg).expect("compile");
        assert!(compiled.lookup_enabled());
        assert!(!compiled.cache_enabled());
        let default = compiled.default_profile().expect("default profile");
        assert_eq!(default.providers, vec![CompiledLookupProvider::Forward]);
    }
}
