//! YAML client configuration for `conduitctl` (control-plane connect defaults).

use anyhow::{anyhow, Context};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:5199";
const CLIENT_CONFIG_FILE: &str = "conduitctl.yaml";

/// Optional durable defaults from a YAML client config file.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ClientFileConfig {
    pub endpoint: Option<String>,
    /// Inline API key (discouraged; prefer `api_key_file`).
    pub api_key: Option<String>,
    pub api_key_file: Option<String>,
    #[serde(default)]
    pub tls: ClientTlsFileConfig,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ClientTlsFileConfig {
    pub ca: Option<String>,
    pub cert: Option<String>,
    pub key: Option<String>,
    #[serde(default)]
    pub insecure_skip_verify: Option<bool>,
}

/// CLI / env / file overrides before built-in defaults.
#[derive(Debug, Clone, Default)]
pub struct ConnectCliOverrides {
    pub config_path: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub api_key_file: Option<PathBuf>,
    pub tls_ca: Option<PathBuf>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// Set when `--tls-insecure` is present on the CLI.
    pub tls_insecure_flag: bool,
}

/// Fully resolved connect settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConnect {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub tls_ca: Option<PathBuf>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub insecure_skip_verify: bool,
    /// Path that was considered for the client file (may be missing).
    pub client_config_path: PathBuf,
    pub client_config_loaded: bool,
}

pub fn default_client_config_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("conduit").join(CLIENT_CONFIG_FILE);
        }
    }
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home)
                .join(".config")
                .join("conduit")
                .join(CLIENT_CONFIG_FILE);
        }
    }
    if let Ok(appdata) = env::var("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata)
                .join("conduit")
                .join(CLIENT_CONFIG_FILE);
        }
    }
    PathBuf::from(CLIENT_CONFIG_FILE)
}

/// Load YAML client config. Missing file → `Ok(None)`.
pub fn load_client_config(path: &Path) -> anyhow::Result<Option<ClientFileConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let yaml = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    let cfg: ClientFileConfig =
        serde_yaml::from_str(&yaml).with_context(|| format!("parsing client config {:?}", path))?;
    Ok(Some(cfg))
}

fn expand_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

fn read_key_file(path: &Path) -> anyhow::Result<String> {
    let key =
        fs::read_to_string(path).with_context(|| format!("reading API key file {:?}", path))?;
    Ok(key.trim().to_string())
}

fn env_bool(name: &str) -> Option<bool> {
    let Ok(raw) = env::var(name) else {
        return None;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).and_then(|v| {
        if v.is_empty() {
            None
        } else {
            Some(PathBuf::from(v))
        }
    })
}

fn env_string(name: &str) -> Option<String> {
    env::var(name).ok().and_then(|v| {
        let t = v.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    })
}

/// Resolve connect settings: flags → env → file → built-ins.
pub fn resolve_connect(overrides: &ConnectCliOverrides) -> anyhow::Result<ResolvedConnect> {
    let client_config_path = overrides
        .config_path
        .clone()
        .or_else(|| env_path("CONDUITCTL_CONFIG"))
        .unwrap_or_else(default_client_config_path);

    let file = load_client_config(&client_config_path)?;
    let loaded = file.is_some();
    let file = file.unwrap_or_default();

    let endpoint = overrides
        .endpoint
        .clone()
        .or_else(|| env_string("CONDUIT_CONTROL"))
        .or_else(|| file.endpoint.clone())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());

    let api_key = if let Some(key) = overrides
        .api_key
        .clone()
        .or_else(|| env_string("CONDUIT_API_KEY"))
    {
        Some(key)
    } else if let Some(path) = overrides
        .api_key_file
        .clone()
        .or_else(|| env_path("CONDUIT_API_KEY_FILE"))
    {
        Some(read_key_file(&path)?)
    } else if let Some(path) = file.api_key_file.as_deref() {
        Some(read_key_file(&expand_path(path))?)
    } else {
        file.api_key.clone()
    };

    let tls_ca = overrides
        .tls_ca
        .clone()
        .or_else(|| env_path("CONDUIT_TLS_CA"))
        .or_else(|| file.tls.ca.as_deref().map(expand_path));

    let tls_cert = overrides
        .tls_cert
        .clone()
        .or_else(|| env_path("CONDUIT_TLS_CERT"))
        .or_else(|| file.tls.cert.as_deref().map(expand_path));

    let tls_key = overrides
        .tls_key
        .clone()
        .or_else(|| env_path("CONDUIT_TLS_KEY"))
        .or_else(|| file.tls.key.as_deref().map(expand_path));

    let insecure_skip_verify = if overrides.tls_insecure_flag {
        true
    } else if let Some(v) = env_bool("CONDUIT_TLS_INSECURE") {
        v
    } else {
        file.tls.insecure_skip_verify.unwrap_or(false)
    };

    if tls_cert.is_some() != tls_key.is_some() {
        return Err(anyhow!(
            "client TLS identity requires both cert and key (got cert={:?} key={:?})",
            tls_cert,
            tls_key
        ));
    }

    Ok(ResolvedConnect {
        endpoint,
        api_key,
        tls_ca,
        tls_cert,
        tls_key,
        insecure_skip_verify,
        client_config_path,
        client_config_loaded: loaded,
    })
}

pub fn default_endpoint() -> &'static str {
    DEFAULT_ENDPOINT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean_env<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        for k in [
            "CONDUIT_CONTROL",
            "CONDUIT_API_KEY",
            "CONDUIT_API_KEY_FILE",
            "CONDUITCTL_CONFIG",
            "CONDUIT_TLS_CA",
            "CONDUIT_TLS_CERT",
            "CONDUIT_TLS_KEY",
            "CONDUIT_TLS_INSECURE",
            "XDG_CONFIG_HOME",
        ] {
            env::remove_var(k);
        }
        f();
    }

    #[test]
    fn missing_file_is_ok_and_uses_builtin_endpoint() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("missing.yaml");
            let resolved = resolve_connect(&ConnectCliOverrides {
                config_path: Some(path.clone()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(resolved.endpoint, DEFAULT_ENDPOINT);
            assert!(!resolved.client_config_loaded);
            assert_eq!(resolved.client_config_path, path);
            assert!(!resolved.insecure_skip_verify);
        });
    }

    #[test]
    fn loads_yaml_endpoint_and_tls_fields() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("conduitctl.yaml");
            fs::write(
                &path,
                r#"
endpoint: https://conduit.example:5199
api_key_file: key.txt
tls:
  ca: /ca.pem
  cert: /client.pem
  key: /client-key.pem
  insecure_skip_verify: true
"#,
            )
            .unwrap();
            fs::write(dir.path().join("key.txt"), "secret-key\n").unwrap();
            // api_key_file in YAML is relative to cwd, not config dir — write absolute:
            fs::write(
                &path,
                format!(
                    r#"
endpoint: https://conduit.example:5199
api_key_file: {}
tls:
  ca: /ca.pem
  cert: /client.pem
  key: /client-key.pem
  insecure_skip_verify: true
"#,
                    dir.path().join("key.txt").display()
                ),
            )
            .unwrap();

            let resolved = resolve_connect(&ConnectCliOverrides {
                config_path: Some(path),
                ..Default::default()
            })
            .unwrap();
            assert!(resolved.client_config_loaded);
            assert_eq!(resolved.endpoint, "https://conduit.example:5199");
            assert_eq!(resolved.api_key.as_deref(), Some("secret-key"));
            assert_eq!(resolved.tls_ca.as_deref(), Some(Path::new("/ca.pem")));
            assert!(resolved.insecure_skip_verify);
        });
    }

    #[test]
    fn flag_overrides_file_endpoint() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("conduitctl.yaml");
            fs::write(&path, "endpoint: https://from-file:5199\n").unwrap();
            let resolved = resolve_connect(&ConnectCliOverrides {
                config_path: Some(path),
                endpoint: Some("https://from-flag:5199".into()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(resolved.endpoint, "https://from-flag:5199");
        });
    }

    #[test]
    fn env_overrides_file_but_not_flag() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("conduitctl.yaml");
            fs::write(&path, "endpoint: https://from-file:5199\n").unwrap();
            env::set_var("CONDUIT_CONTROL", "https://from-env:5199");
            let from_env = resolve_connect(&ConnectCliOverrides {
                config_path: Some(path.clone()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(from_env.endpoint, "https://from-env:5199");

            let from_flag = resolve_connect(&ConnectCliOverrides {
                config_path: Some(path),
                endpoint: Some("https://from-flag:5199".into()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(from_flag.endpoint, "https://from-flag:5199");
            env::remove_var("CONDUIT_CONTROL");
        });
    }

    #[test]
    fn tls_insecure_flag_overrides_file_false() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("conduitctl.yaml");
            fs::write(&path, "tls:\n  insecure_skip_verify: false\n").unwrap();
            let resolved = resolve_connect(&ConnectCliOverrides {
                config_path: Some(path),
                tls_insecure_flag: true,
                ..Default::default()
            })
            .unwrap();
            assert!(resolved.insecure_skip_verify);
        });
    }

    #[test]
    fn default_path_uses_xdg_when_set() {
        with_clean_env(|| {
            let dir = tempfile::tempdir().unwrap();
            env::set_var("XDG_CONFIG_HOME", dir.path());
            let path = default_client_config_path();
            assert_eq!(path, dir.path().join("conduit").join(CLIENT_CONFIG_FILE));
            env::remove_var("XDG_CONFIG_HOME");
        });
    }
}
