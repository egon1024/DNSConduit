//! Cache entry metadata and classification.

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Stored answer classification (single heterogeneous pool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Positive,
    NxDomain,
    NoData,
    ServFail,
}

impl EntryKind {
    pub fn from_rcode(rcode: u16, answer_count: u16) -> Self {
        match rcode {
            0 if answer_count == 0 => Self::NoData,
            0 => Self::Positive,
            3 => Self::NxDomain,
            2 => Self::ServFail,
            _ => Self::Positive,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub kind: EntryKind,
    pub wire: Arc<[u8]>,
    /// Wall-clock instant when this entry was stored (used to age RR TTLs on serve).
    pub filled_at: Instant,
    pub expires_at: Instant,
}

impl CacheEntry {
    pub fn is_fresh(&self, now: Instant) -> bool {
        now < self.expires_at
    }

    pub fn ttl_remaining_secs(&self, now: Instant) -> u32 {
        if now >= self.expires_at {
            return 0;
        }
        self.expires_at
            .duration_since(now)
            .as_secs()
            .min(u32::MAX as u64) as u32
    }
}

pub fn expires_at_from_ttl(now: Instant, ttl_secs: u32) -> Instant {
    now + Duration::from_secs(u64::from(ttl_secs))
}
