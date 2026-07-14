//! Embedded semantic cache — redb backend with bbolt-compatible TTL wire format.

pub mod encoding;
pub mod entry;
pub mod eviction;
pub mod tool;
pub mod vector;
pub mod semantic;
pub mod store;
pub mod redis_store;

pub use encoding::{decode_stored_value, encode_stored_value, expiry_prefix_len, expires_at_nano};
pub use entry::Entry;
pub use eviction::start_eviction_worker;
pub use semantic::{generate_cache_key, key_for_request, parse_cache_key_strategy, CacheKeyStrategy};
pub use store::{Store, StoreError, StoreOptions};
