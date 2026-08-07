//! DNS answer cache (memory/LMDB backends, single-flight, fill on forward).

mod backend;
mod entry;
mod inflight;
mod key;
mod lmdb;
mod memory;
mod registry;
mod serve;

pub use backend::CacheBackend;
pub use entry::{CacheEntry, EntryKind};
pub use inflight::{InFlightRole, InFlightTable};
pub use key::{build_query_key, build_truncated_udp_key, CacheKey, TransportKey};
pub use lmdb::{LmdbBackendError, LmdbCacheBackend, FORMAT_VERSION as LMDB_FORMAT_VERSION};
pub use memory::{entry_from_wire, MemoryCacheBackend, ReapBudget, ReapCursor, ReapOutcome};
pub use registry::{CacheLookupOutcome, CacheWaitWake, LookupCacheRegistry};
pub use serve::prepare_served_wire;
