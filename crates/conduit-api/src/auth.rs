//! Control-plane authentication (design §8.2).
//!
//! v1 ships built-in API keys and optional mTLS on the control listener. Dynamic
//! `AuthProvider` plugins are deferred; implementors can replace this layer post–v1.

use conduit_core::SnapshotStore;
use tonic::Request;

/// Extension point for future auth plugins (design §8.2). Not loaded dynamically in v1.
pub trait AuthProvider: Send + Sync {
    #[allow(clippy::result_large_err)]
    fn authorize(&self, request: &Request<()>) -> Result<(), tonic::Status>;
}

/// Built-in API key validation from the active snapshot.
pub struct ApiKeyAuth {
    snapshots: Arc<SnapshotStore>,
}

impl AuthProvider for ApiKeyAuth {
    fn authorize(&self, request: &Request<()>) -> Result<(), tonic::Status> {
        authorize_api_keys(&self.snapshots, request.metadata())
    }
}
use std::sync::Arc;
use tonic::Status;

/// Validates API keys from request metadata when keys are configured on the active snapshot.
#[derive(Clone)]
pub struct ApiKeyInterceptor {
    snapshots: Arc<SnapshotStore>,
}

impl ApiKeyInterceptor {
    pub fn new(snapshots: Arc<SnapshotStore>) -> Self {
        Self { snapshots }
    }
}

impl tonic::service::Interceptor for ApiKeyInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        authorize_api_keys(&self.snapshots, request.metadata())?;
        Ok(request)
    }
}

#[allow(clippy::result_large_err)]
fn authorize_api_keys(
    snapshots: &SnapshotStore,
    meta: &tonic::metadata::MetadataMap,
) -> Result<(), Status> {
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

    if let Some(token) = bearer_token(meta) {
        if keys.iter().any(|k| k == token) {
            return Ok(());
        }
    }
    if let Some(key) = header_value(meta, "x-api-key") {
        if keys.iter().any(|k| k == key) {
            return Ok(());
        }
    }

    Err(Status::unauthenticated(
        "missing or invalid API key (use Authorization: Bearer <key> or x-api-key)",
    ))
}

fn bearer_token(meta: &tonic::metadata::MetadataMap) -> Option<&str> {
    let value = header_value(meta, "authorization")?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

fn header_value<'a>(meta: &'a tonic::metadata::MetadataMap, name: &str) -> Option<&'a str> {
    meta.get(name)?.to_str().ok()
}
