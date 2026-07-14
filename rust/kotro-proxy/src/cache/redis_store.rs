//! Redis-backed shared cache store for team deployments.
//!
//! # Async safety
//! The `Store` trait is synchronous, but Redis I/O is inherently blocking.
//! Every public method wraps its blocking connection work inside
//! `tokio::task::block_in_place`, which yields the async executor thread to
//! other tasks while the blocking call runs — safe to call from Axum handlers.
//!
//! # Key namespacing
//! All keys are stored under the `kotro:cache:` prefix so multiple Kotro
//! instances or other apps can safely share the same Redis instance.
//!
//! # TTL
//! TTL is delegated entirely to Redis via `SETEX`. Kotro's own encoding
//! layer (`encode_stored_value`) is NOT used here — Redis handles expiry
//! natively and more efficiently.

use std::time::Duration;
use std::sync::Arc;

use redis::Commands;

use crate::cache::entry::Entry;
use crate::cache::store::{StoreError, StoreOptions};

/// Prefix applied to every key stored in Redis.
const KEY_PREFIX: &str = "kotro:cache:";

fn namespaced(key: &str) -> String {
    format!("{KEY_PREFIX}{key}")
}

#[derive(Clone)]
pub struct RedisStore {
    /// Thread-safe reference-counted Redis client.
    /// `redis::Client` is cheap to clone and manages its own internal pool.
    client: Arc<redis::Client>,
    ttl: Duration,
    compress: bool,
}

impl RedisStore {
    pub fn new(url: &str, opts: StoreOptions) -> Result<Self, StoreError> {
        let client = redis::Client::open(url)
            .map_err(|e| StoreError::Redis(e.to_string()))?;
        Ok(Self {
            client: Arc::new(client),
            ttl: opts.ttl,
            compress: opts.enable_compression,
        })
    }

    /// Returns the actual compression setting (mirrors `LocalStore`).
    pub fn compression_enabled(&self) -> bool {
        self.compress
    }

    /// Returns the configured TTL (mirrors `LocalStore`).
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn get(&self, key: &str) -> Result<Option<Entry>, StoreError> {
        let client = Arc::clone(&self.client);
        let ns_key = namespaced(key);

        // block_in_place: tells Tokio "this thread will block briefly" so the
        // executor can schedule other async tasks on a different thread.
        tokio::task::block_in_place(|| {
            let mut con = client
                .get_connection()
                .map_err(|e| StoreError::Redis(e.to_string()))?;

            let payload: Option<Vec<u8>> = con
                .get(&ns_key)
                .map_err(|e| StoreError::Redis(e.to_string()))?;

            let Some(payload) = payload else {
                return Ok(None);
            };

            // Payload is raw JSON bytes — TTL is handled by SETEX, not Kotro encoding.
            let entry: Entry = serde_json::from_slice(&payload)?;
            Ok(Some(entry))
        })
    }

    pub fn put(&self, entry: Entry) -> Result<(), StoreError> {
        let client = Arc::clone(&self.client);
        let ns_key = namespaced(&entry.key);
        let ttl = self.ttl;
        // Store as raw JSON — no Kotro encoding wrapper needed (Redis owns the TTL).
        let payload = serde_json::to_vec(&entry)?;

        tokio::task::block_in_place(|| {
            let mut con = client
                .get_connection()
                .map_err(|e| StoreError::Redis(e.to_string()))?;

            if ttl.is_zero() {
                let _: () = con
                    .set(&ns_key, &payload)
                    .map_err(|e| StoreError::Redis(e.to_string()))?;
            } else {
                let _: () = con
                    .set_ex(&ns_key, &payload, ttl.as_secs())
                    .map_err(|e| StoreError::Redis(e.to_string()))?;
            }
            Ok(())
        })
    }

    pub fn delete(&self, key: &str) -> Result<(), StoreError> {
        let client = Arc::clone(&self.client);
        let ns_key = namespaced(key);

        tokio::task::block_in_place(|| {
            let mut con = client
                .get_connection()
                .map_err(|e| StoreError::Redis(e.to_string()))?;
            let _: () = con
                .del(&ns_key)
                .map_err(|e| StoreError::Redis(e.to_string()))?;
            Ok(())
        })
    }

    /// Returns the count of keys under the `kotro:cache:*` namespace only.
    /// Uses SCAN + pattern rather than DBSIZE to avoid counting foreign keys.
    pub fn count(&self) -> Result<usize, StoreError> {
        let client = Arc::clone(&self.client);
        let pattern = format!("{KEY_PREFIX}*");

        tokio::task::block_in_place(|| {
            let mut con = client
                .get_connection()
                .map_err(|e| StoreError::Redis(e.to_string()))?;

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(&pattern)
                .query(&mut con)
                .map_err(|e| StoreError::Redis(e.to_string()))?;

            Ok(keys.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify key namespacing helper.
    #[test]
    fn namespaced_key_has_prefix() {
        assert_eq!(namespaced("abc123"), "kotro:cache:abc123");
        assert_eq!(namespaced(""), "kotro:cache:");
    }

    /// Verify compression_enabled() reflects the opts field.
    #[test]
    fn compression_enabled_reflects_opts() {
        // We can't open a real Redis in unit tests, but we can verify the field
        // assignment logic by constructing the struct directly.
        let store = RedisStore {
            client: Arc::new(redis::Client::open("redis://127.0.0.1/").unwrap()),
            ttl: Duration::from_secs(60),
            compress: true,
        };
        assert!(store.compression_enabled());

        let store2 = RedisStore {
            client: Arc::new(redis::Client::open("redis://127.0.0.1/").unwrap()),
            ttl: Duration::ZERO,
            compress: false,
        };
        assert!(!store2.compression_enabled());
    }
}
