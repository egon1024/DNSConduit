//! Control-plane authentication (design §8.2).
//!
//! v1 ships built-in API keys and optional mTLS on the control listener. Dynamic
//! `AuthProvider` plugins are deferred; implementors can replace this layer post–v1.

use conduit_core::SnapshotStore;
use std::sync::Arc;
use tonic::metadata::MetadataMap;
use tonic::transport::server::TcpConnectInfo;
use tonic::transport::server::TlsConnectInfo;
use tonic::{Request, Status};

/// Extension point for future auth plugins (design §8.2). Not loaded dynamically in v1.
pub trait AuthProvider: Send + Sync {
    #[allow(clippy::result_large_err)]
    fn authorize(&self, request: &Request<()>) -> Result<(), Status>;
}

/// Built-in API key validation from the active snapshot.
pub struct ApiKeyAuth {
    snapshots: Arc<SnapshotStore>,
}

impl AuthProvider for ApiKeyAuth {
    fn authorize(&self, request: &Request<()>) -> Result<(), Status> {
        authorize_api_keys(&self.snapshots, request.metadata())
    }
}

/// Validates API keys from request metadata when keys are configured on the active snapshot.
#[derive(Clone)]
pub struct ControlInterceptor {
    snapshots: Arc<SnapshotStore>,
}

impl ControlInterceptor {
    pub fn new(snapshots: Arc<SnapshotStore>) -> Self {
        Self { snapshots }
    }
}

impl tonic::service::Interceptor for ControlInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        if let Err(status) = authorize_api_keys(&self.snapshots, request.metadata()) {
            crate::access_log::log_interceptor_denial(
                &self.snapshots,
                request.metadata(),
                request.extensions(),
                &status,
            );
            return Err(status);
        }
        Ok(request)
    }
}

/// Requestor identity for access logs (no secrets; no payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestorKind {
    /// No API keys configured on the control listener.
    Anonymous,
    /// Valid API key (key value is never logged).
    ApiKey,
    /// API keys required but missing or wrong.
    ApiKeyRejected,
    /// Client presented a TLS peer certificate (mTLS).
    Mtls,
    /// API keys required and no credentials presented.
    Unauthenticated,
}

impl RequestorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestorKind::Anonymous => "anonymous",
            RequestorKind::ApiKey => "api_key",
            RequestorKind::ApiKeyRejected => "api_key_rejected",
            RequestorKind::Mtls => "mtls",
            RequestorKind::Unauthenticated => "unauthenticated",
        }
    }
}

/// Classify the caller for access logging.
pub fn requestor_label(
    snapshots: &SnapshotStore,
    meta: &MetadataMap,
    extensions: &http::Extensions,
    peer_certs_present: bool,
) -> String {
    if peer_certs_present
        || extensions
            .get::<TlsConnectInfo<TcpConnectInfo>>()
            .and_then(|info| info.peer_certs())
            .is_some()
    {
        return RequestorKind::Mtls.as_str().into();
    }

    let snap = snapshots.load();
    let keys: &[String] = snap
        .config
        .control
        .as_ref()
        .map(|c| c.api_keys.as_slice())
        .unwrap_or(&[]);

    if keys.is_empty() {
        return RequestorKind::Anonymous.as_str().into();
    }

    if api_key_matches(keys, meta) {
        return RequestorKind::ApiKey.as_str().into();
    }

    if has_api_key_credential(meta) {
        return RequestorKind::ApiKeyRejected.as_str().into();
    }

    RequestorKind::Unauthenticated.as_str().into()
}

#[allow(clippy::result_large_err)]
pub fn authorize_api_keys(snapshots: &SnapshotStore, meta: &MetadataMap) -> Result<(), Status> {
    let snap = snapshots.load();
    let keys: &[String] = snap
        .config
        .control
        .as_ref()
        .map(|c| c.api_keys.as_slice())
        .unwrap_or(&[]);
    if keys.is_empty() {
        return Ok(());
    }

    if api_key_matches(keys, meta) {
        return Ok(());
    }

    Err(Status::unauthenticated(
        "missing or invalid API key (use Authorization: Bearer <key> or x-api-key)",
    ))
}

fn api_key_matches(keys: &[String], meta: &MetadataMap) -> bool {
    if let Some(token) = bearer_token(meta) {
        if keys.iter().any(|k| k == token) {
            return true;
        }
    }
    if let Some(key) = header_value(meta, "x-api-key") {
        if keys.iter().any(|k| k == key) {
            return true;
        }
    }
    false
}

fn has_api_key_credential(meta: &MetadataMap) -> bool {
    bearer_token(meta).is_some() || header_value(meta, "x-api-key").is_some()
}

fn bearer_token(meta: &MetadataMap) -> Option<&str> {
    let value = header_value(meta, "authorization")?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

fn header_value<'a>(meta: &'a MetadataMap, name: &str) -> Option<&'a str> {
    meta.get(name)?.to_str().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    use conduit_core::{RuntimeSnapshot, SnapshotStore};

    fn store_with_keys(keys: Vec<&str>) -> Arc<SnapshotStore> {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        let control = cfg
            .control
            .get_or_insert_with(|| conduit_proto::config::ControlConfig {
                listen_address: "127.0.0.1:5199".into(),
                reflection_enabled: false,
                api_keys: vec![],
                tls: None,
            });
        control.api_keys = keys.into_iter().map(String::from).collect();
        Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(cfg)))
    }

    #[test]
    fn requestor_anonymous_when_no_keys() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(cfg)));
        let meta = MetadataMap::new();
        assert_eq!(
            requestor_label(&store, &meta, &http::Extensions::new(), false),
            "anonymous"
        );
    }

    #[test]
    fn requestor_api_key_when_valid() {
        let store = store_with_keys(vec!["secret"]);
        let mut meta = MetadataMap::new();
        meta.insert(
            "authorization",
            tonic::metadata::MetadataValue::try_from("Bearer secret").unwrap(),
        );
        assert_eq!(
            requestor_label(&store, &meta, &http::Extensions::new(), false),
            "api_key"
        );
    }

    #[test]
    fn requestor_rejected_when_wrong_key() {
        let store = store_with_keys(vec!["secret"]);
        let mut meta = MetadataMap::new();
        meta.insert(
            "x-api-key",
            tonic::metadata::MetadataValue::try_from("wrong").unwrap(),
        );
        assert_eq!(
            requestor_label(&store, &meta, &http::Extensions::new(), false),
            "api_key_rejected"
        );
    }
}
