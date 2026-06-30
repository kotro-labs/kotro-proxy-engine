//! Embedded semantic cache — redb backend with bbolt-compatible TTL wire format.

pub mod encoding;
pub mod entry;
pub mod semantic;
pub mod store;

pub use encoding::{decode_stored_value, encode_stored_value, expiry_prefix_len, expires_at_nano};
pub use entry::Entry;
pub use semantic::key_for_request;
pub use store::{Store, StoreError, StoreOptions};
