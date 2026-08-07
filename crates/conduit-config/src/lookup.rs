//! Compiled lookup profiles and named cache instances (pluggable-lookup-system).

use conduit_proto::config::{
    CacheInstance, CacheLmdbConfig, CacheMemoryConfig, CacheNegativeConfig, CacheOnHitConfig,
    CacheTruncatedUdpConfig, Config, LookupProfile, LookupProvider,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_LOOKUP_PROFILE: &str = "default";
pub const DEFAULT_SERVFAIL_TTL_SECS: u32 = 10;
pub const DEFAULT_TRUNCATED_UDP_TTL_SECS: u32 = 60;
pub const DEFAULT_ON_HIT_RESPONSE_RULES: &str = "skip";
pub const DEFAULT_EVICTION_MODE: &str = "passive";
pub const DEFAULT_MEMORY_SHARD_COUNT: u32 = 16;
pub const DEFAULT_LMDB_SAMPLE_SIZE: u32 = 16;
pub const DEFAULT_LMDB_WHEN_FULL: &str = "evict_one";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheBackendType {
    Memory,
    Lmdb,
    /// Reserved — not implemented.
    EbpfMap,
}

impl CacheBackendType {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "memory" => Ok(Self::Memory),
            "lmdb" => Ok(Self::Lmdb),
            "ebpf_map" => Ok(Self::EbpfMap),
            other => Err(format!(
                "cache type '{other}' must be memory or lmdb (ebpf_map is reserved)"
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LmdbWhenFull {
    Refuse,
    EvictOne,
    Sample,
}

impl LmdbWhenFull {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "" | "evict_one" => Ok(Self::EvictOne),
            "refuse" => Ok(Self::Refuse),
            "sample" => Ok(Self::Sample),
            other => Err(format!(
                "lmdb.when_full '{other}' must be refuse, evict_one, or sample"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::EvictOne => "evict_one",
            Self::Sample => "sample",
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
pub struct CompiledLmdbCache {
    pub path: PathBuf,
    pub map_size_bytes: u64,
    pub when_full: LmdbWhenFull,
    pub sample_size: u32,
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
    pub lmdb: Option<CompiledLmdbCache>,
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

        match backend_type {
            CacheBackendType::EbpfMap => {
                return Err(format!(
                    "{ctx}: type 'ebpf_map' is reserved and not implemented"
                ));
            }
            CacheBackendType::Memory => {
                if instance.lmdb.is_some() {
                    return Err(format!(
                        "{ctx}: lmdb block is not allowed when type is memory"
                    ));
                }
            }
            CacheBackendType::Lmdb => {
                if instance.memory.is_some() {
                    return Err(format!(
                        "{ctx}: memory block is not allowed when type is lmdb"
                    ));
                }
            }
        }

        let negative_cache = compile_negative_cache(instance.negative_cache.as_ref())?;
        let on_hit_response_rules = compile_on_hit(instance.on_hit.as_ref(), &ctx)?;
        let truncated_udp = compile_truncated_udp(instance.truncated_udp.as_ref(), &ctx)?;

        let (memory, lmdb) = match backend_type {
            CacheBackendType::Memory => {
                let memory = compile_memory(instance.memory.as_ref(), &ctx)?;
                (memory, None)
            }
            CacheBackendType::Lmdb => {
                let lmdb = compile_lmdb(instance.lmdb.as_ref(), &ctx)?;
                (
                    CompiledMemoryCache {
                        shard_count: DEFAULT_MEMORY_SHARD_COUNT,
                        eviction: EvictionMode::Passive,
                    },
                    Some(lmdb),
                )
            }
            CacheBackendType::EbpfMap => unreachable!("rejected above"),
        };

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
                lmdb,
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

fn compile_lmdb(cfg: Option<&CacheLmdbConfig>, ctx: &str) -> Result<CompiledLmdbCache, String> {
    let Some(l) = cfg else {
        return Err(format!(
            "{ctx}: type lmdb requires an lmdb block with path and map_size"
        ));
    };
    if l.path.is_empty() {
        return Err(format!("{ctx}.lmdb.path must not be empty"));
    }
    if l.map_size_bytes == 0 {
        return Err(format!("{ctx}.lmdb.map_size must be set and > 0"));
    }

    let when_full = LmdbWhenFull::parse(l.when_full.as_deref().unwrap_or(DEFAULT_LMDB_WHEN_FULL))
        .map_err(|e| format!("{ctx}.lmdb: {e}"))?;
    let sample_size = l.sample_size.unwrap_or(DEFAULT_LMDB_SAMPLE_SIZE);
    if when_full == LmdbWhenFull::Sample && sample_size < 1 {
        return Err(format!(
            "{ctx}.lmdb.sample_size must be >= 1 when when_full is sample"
        ));
    }

    let path = PathBuf::from(&l.path);
    preflight_lmdb_path(&path, ctx)?;

    Ok(CompiledLmdbCache {
        path,
        map_size_bytes: l.map_size_bytes,
        when_full,
        sample_size,
    })
}

/// When the path already exists it must be a readable/writable directory.
/// When it does not, the nearest existing ancestor (if any) must be a writable
/// directory so open can create the env path. Missing path components are
/// created at open time — validate does not mkdir.
fn preflight_lmdb_path(path: &Path, ctx: &str) -> Result<(), String> {
    if path.exists() {
        let meta = fs::metadata(path)
            .map_err(|e| format!("{ctx}.lmdb.path is not readable ({}): {e}", path.display()))?;
        if !meta.is_dir() {
            return Err(format!(
                "{ctx}.lmdb.path must be a directory (got '{}')",
                path.display()
            ));
        }
        if meta.permissions().readonly() {
            return Err(format!(
                "{ctx}.lmdb.path is not writable: {}",
                path.display()
            ));
        }
        let probe = path.join(".conduit-lmdb-preflight");
        match fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&probe)
        {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
            }
            Err(e) => {
                return Err(format!(
                    "{ctx}.lmdb.path must be readable and writable ({}): {e}",
                    path.display()
                ));
            }
        }
        return Ok(());
    }

    // Path does not exist yet — require a writable directory ancestor when one exists.
    let mut ancestor = path.parent();
    while let Some(p) = ancestor {
        if p.as_os_str().is_empty() {
            break;
        }
        if p.exists() {
            if !p.is_dir() {
                return Err(format!(
                    "{ctx}.lmdb.path ancestor is not a directory: {}",
                    p.display()
                ));
            }
            let probe = p.join(".conduit-lmdb-preflight");
            match fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&probe)
            {
                Ok(_) => {
                    let _ = fs::remove_file(&probe);
                }
                Err(e) => {
                    return Err(format!(
                        "{ctx}.lmdb.path ancestor is not writable ({}): {e}",
                        p.display()
                    ));
                }
            }
            return Ok(());
        }
        ancestor = p.parent();
    }

    Ok(())
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
        CacheInstance, CacheLmdbConfig, CacheOnHitConfig, CacheTruncatedUdpConfig, LookupConfig,
        LookupProfile, LookupProvider,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn minimal_with_lookup(lookup: LookupConfig, caches: Vec<CacheInstance>) -> Config {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).expect("minimal");
        cfg.lookup = Some(lookup);
        cfg.caches = caches;
        cfg
    }

    fn memory_cache(name: &str) -> CacheInstance {
        CacheInstance {
            name: name.into(),
            r#type: "memory".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            lmdb: None,
            key: None,
            max_entries: None,
        }
    }

    fn unique_lmdb_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("conduit-lmdb-test-{nanos}"))
    }

    fn cache_profile(cache_name: &str) -> LookupConfig {
        LookupConfig {
            profiles: HashMap::from([(
                "default".into(),
                LookupProfile {
                    providers: vec![LookupProvider {
                        r#type: "cache".into(),
                        cache: Some(cache_name.into()),
                    }],
                },
            )]),
        }
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
        let lookup = cache_profile("global");
        let err =
            CompiledLookup::compile_from_config(&minimal_with_lookup(lookup, vec![])).unwrap_err();
        assert!(err.contains("caches list is empty"));
    }

    #[test]
    fn invalid_cache_ref_rejected() {
        let lookup = cache_profile("missing");
        let caches = vec![memory_cache("global")];
        let err =
            CompiledLookup::compile_from_config(&minimal_with_lookup(lookup, caches)).unwrap_err();
        assert!(err.contains("unknown cache instance 'missing'"));
    }

    #[test]
    fn on_hit_defaults_to_run() {
        let caches = vec![memory_cache("global")];
        let compiled = CompiledLookup::compile_from_config(&minimal_with_lookup(
            cache_profile("global"),
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
        let mut caches = vec![memory_cache("global")];
        caches[0].on_hit = Some(CacheOnHitConfig {
            response_rules: "maybe".into(),
        });
        let err = CompiledLookup::compile_from_config(&minimal_with_lookup(
            cache_profile("global"),
            caches,
        ))
        .unwrap_err();
        assert!(err.contains("on_hit.response_rules"));
    }

    #[test]
    fn truncated_udp_ttl_required_when_enabled() {
        let mut caches = vec![memory_cache("global")];
        caches[0].truncated_udp = Some(CacheTruncatedUdpConfig {
            enabled: Some(true),
            ttl_secs: None,
        });
        let err = CompiledLookup::compile_from_config(&minimal_with_lookup(
            cache_profile("global"),
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

    #[test]
    fn lmdb_instance_compiles_with_defaults() {
        let path = unique_lmdb_path();
        let caches = vec![CacheInstance {
            name: "durable".into(),
            r#type: "lmdb".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            lmdb: Some(CacheLmdbConfig {
                path: path.to_string_lossy().into(),
                map_size_bytes: 1_000_000_000,
                when_full: None,
                sample_size: None,
            }),
            key: None,
            max_entries: Some(1000),
        }];
        let compiled = CompiledLookup::compile_from_config(&minimal_with_lookup(
            cache_profile("durable"),
            caches,
        ))
        .expect("compile");
        let inst = &compiled.cache_instances["durable"];
        assert_eq!(inst.backend_type, CacheBackendType::Lmdb);
        let lmdb = inst.lmdb.as_ref().expect("lmdb");
        assert_eq!(lmdb.map_size_bytes, 1_000_000_000);
        assert_eq!(lmdb.when_full, LmdbWhenFull::EvictOne);
        assert_eq!(lmdb.sample_size, DEFAULT_LMDB_SAMPLE_SIZE);
    }

    #[test]
    fn lmdb_missing_block_rejected() {
        let caches = vec![CacheInstance {
            name: "durable".into(),
            r#type: "lmdb".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            lmdb: None,
            key: None,
            max_entries: None,
        }];
        let err = CompiledLookup::compile_from_config(&minimal_with_lookup(
            cache_profile("durable"),
            caches,
        ))
        .unwrap_err();
        assert!(err.contains("requires an lmdb block"));
    }

    fn sparse_lmdb_yaml(cache_block: &str) -> String {
        format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
caches:
{cache_block}
lookup:
  profiles:
    default:
      providers:
        - type: cache
          cache: durable
"#
        )
    }

    #[test]
    fn lmdb_foreign_memory_block_rejected() {
        let path = unique_lmdb_path();
        let yaml = sparse_lmdb_yaml(&format!(
            r#"  - name: durable
    type: lmdb
    memory:
      shard_count: 4
    lmdb:
      path: {path}
      map_size: 1GB"#,
            path = path.display()
        ));
        let cfg = load_yaml(&yaml).expect("parse");
        let err = CompiledLookup::compile_from_config(&cfg).unwrap_err();
        assert!(err.contains("memory block is not allowed"));
    }

    #[test]
    fn memory_foreign_lmdb_block_rejected() {
        let path = unique_lmdb_path();
        let yaml = sparse_lmdb_yaml(&format!(
            r#"  - name: durable
    type: memory
    lmdb:
      path: {path}
      map_size: 1GB"#,
            path = path.display()
        ));
        let cfg = load_yaml(&yaml).expect("parse");
        let err = CompiledLookup::compile_from_config(&cfg).unwrap_err();
        assert!(err.contains("lmdb block is not allowed"));
    }

    #[test]
    fn ebpf_map_still_rejected() {
        let caches = vec![CacheInstance {
            name: "x".into(),
            r#type: "ebpf_map".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            lmdb: None,
            key: None,
            max_entries: None,
        }];
        let err =
            CompiledLookup::compile_from_config(&minimal_with_lookup(cache_profile("x"), caches))
                .unwrap_err();
        assert!(err.contains("ebpf_map"));
        assert!(err.contains("reserved"));
    }

    #[test]
    fn lmdb_path_that_is_a_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("not-a-dir");
        std::fs::write(&file_path, b"x").unwrap();
        let caches = vec![CacheInstance {
            name: "durable".into(),
            r#type: "lmdb".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            lmdb: Some(CacheLmdbConfig {
                path: file_path.to_string_lossy().into_owned(),
                map_size_bytes: 1_000_000,
                when_full: None,
                sample_size: None,
            }),
            key: None,
            max_entries: None,
        }];
        let err = CompiledLookup::compile_from_config(&minimal_with_lookup(
            cache_profile("durable"),
            caches,
        ))
        .unwrap_err();
        assert!(
            err.contains("must be a directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn lmdb_missing_path_compiles_when_ancestor_writable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("env");
        assert!(!path.exists());
        let caches = vec![CacheInstance {
            name: "durable".into(),
            r#type: "lmdb".into(),
            negative_cache: None,
            on_hit: None,
            truncated_udp: None,
            rotate_rrset_on_serve: None,
            memory: None,
            lmdb: Some(CacheLmdbConfig {
                path: path.to_string_lossy().into_owned(),
                map_size_bytes: 1_000_000,
                when_full: None,
                sample_size: None,
            }),
            key: None,
            max_entries: None,
        }];
        CompiledLookup::compile_from_config(&minimal_with_lookup(cache_profile("durable"), caches))
            .expect("missing nested path should compile; open creates directories");
        assert!(
            !path.exists(),
            "validate/compile must not create the env path"
        );
    }

    #[test]
    fn map_size_si_yaml_parses() {
        let path = unique_lmdb_path();
        let yaml = sparse_lmdb_yaml(&format!(
            r#"  - name: durable
    type: lmdb
    lmdb:
      path: {path}
      map_size: 4.5GB"#,
            path = path.display()
        ));
        let cfg = load_yaml(&yaml).expect("parse");
        assert_eq!(
            cfg.caches[0].lmdb.as_ref().unwrap().map_size_bytes,
            4_500_000_000
        );
        let compiled = CompiledLookup::compile_from_config(&cfg).expect("compile");
        assert_eq!(
            compiled.cache_instances["durable"]
                .lmdb
                .as_ref()
                .unwrap()
                .map_size_bytes,
            4_500_000_000
        );
    }

    #[test]
    fn map_size_iec_yaml_rejected_at_load() {
        let path = unique_lmdb_path();
        let yaml = sparse_lmdb_yaml(&format!(
            r#"  - name: durable
    type: lmdb
    lmdb:
      path: {path}
      map_size: 4GiB"#,
            path = path.display()
        ));
        let err = load_yaml(&yaml).unwrap_err().to_string();
        assert!(err.contains("IEC") || err.contains("binary"), "{err}");
    }
}
