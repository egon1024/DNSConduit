//! Cooperative shutdown signaling for listener workers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cooperative shutdown flag shared with listener worker threads.
#[derive(Clone)]
pub struct DataplaneShutdown(Arc<AtomicBool>);

impl Default for DataplaneShutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl DataplaneShutdown {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn is_shutdown(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub(crate) fn signal(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
