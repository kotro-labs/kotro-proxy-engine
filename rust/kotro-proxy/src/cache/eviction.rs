#![allow(clippy::result_large_err)]
//! Background TTL sweeps — mirrors `internal/cache/eviction.go`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redb::{Database, TableError, ReadableTable, ReadableTableMetadata};
use tokio::time::interval;
use tracing::{error, info};

use super::encoding::EXPIRY_PREFIX_LEN;
use super::store::{Store, StoreError, CACHE_TABLE};

/// Returns `true` when an entry should be kept (legacy `{` prefix or not yet expired).
pub fn should_retain_entry(raw: &[u8], now_nano: i64) -> bool {
    if raw.is_empty() || raw[0] == b'{' {
        return true;
    }
    if raw.len() < EXPIRY_PREFIX_LEN {
        return true;
    }
    let expires_at = u64::from_be_bytes(raw[..EXPIRY_PREFIX_LEN].try_into().unwrap()) as i64;
    if expires_at <= 0 {
        return true;
    }
    now_nano <= expires_at
}

fn now_unix_nano() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64
}

/// Sweeps stale keys in a single write transaction via redb's in-place `.retain()` filter,
/// and enforces max capacity bounds.
pub fn sweep_stale(db: &Database, now_nano: i64, max_capacity: Option<usize>) -> Result<usize, StoreError> {
    let write_txn = db.begin_write()?;
    let deleted = {
        let mut table = match write_txn.open_table(CACHE_TABLE) {
            Ok(t) => t,
            Err(TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e.into()),
        };

        let mut deleted = 0usize;
        table.retain(|_key, value| {
            let keep = should_retain_entry(value, now_nano);
            if !keep {
                deleted += 1;
            }
            keep
        })?;

        if let Some(capacity) = max_capacity {
            let len = table.len()? as usize;
            if len > capacity {
                let to_drop = len - capacity;
                let mut keys_to_remove = Vec::with_capacity(to_drop);
                for (i, entry) in table.iter()?.enumerate() {
                    if i >= to_drop {
                        break;
                    }
                    if let Ok((k, _v)) = entry {
                        keys_to_remove.push(k.value().to_string());
                    }
                }
                for k in keys_to_remove {
                    table.remove(k.as_str())?;
                    deleted += 1;
                }
            }
        }

        deleted
    };
    write_txn.commit()?;
    Ok(deleted)
}

/// Spawns a periodic eviction loop until the tokio runtime shuts down.
pub fn start_eviction_worker(store: Store, sweep_interval: Duration) {
    if (store.ttl().is_zero() && store.max_capacity().is_none()) || sweep_interval.is_zero() {
        return;
    }

    tokio::spawn(async move {
        let mut ticker = interval(sweep_interval);
        ticker.tick().await;

        loop {
            ticker.tick().await;
            match store.sweep_expired() {
                Ok(n) if n > 0 => tracing::debug!(evicted = n, "swept stale cache entries"),
                Err(e) => tracing::error!("cache sweep failed: {e}"),
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{encoding::encode_stored_value, entry::Entry, StoreOptions};
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn retains_legacy_and_active_entries() {
        let legacy = br#"{"Key":"k"}"#;
        assert!(should_retain_entry(legacy, i64::MAX));

        let active = encode_stored_value(9_999_999_999_999_999, br#"{"Key":"k"}"#, false);
        assert!(should_retain_entry(&active, 1));

        let expired = encode_stored_value(100, br#"{"Key":"k"}"#, false);
        assert!(!should_retain_entry(&expired, 200));
    }

    #[test]
    fn retain_sweep_matches_go_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_with_options(
            dir.path().join("evict.db"),
            StoreOptions {
                ttl: StdDuration::from_millis(1),
                ..Default::default()
            },
        )
        .unwrap();

        store
            .put(Entry {
                key: "gone".into(),
                raw_sse: b"data: y\n\n".to_vec(),
                model: "gpt-4".into(),
                created_at: 0,
            })
            .unwrap();

        let legacy_entry = Entry {
            key: "legacy".into(),
            raw_sse: b"data: stay\n\n".to_vec(),
            model: "gpt-4".into(),
            created_at: 0,
        };
        store
            .put_raw("legacy", &serde_json::to_vec(&legacy_entry).unwrap())
            .unwrap();

        thread::sleep(StdDuration::from_millis(5));

        let deleted = store.sweep_expired().unwrap();
        assert_eq!(deleted, 1);
        assert!(store.get("gone").unwrap().is_none());
        assert!(store.get("legacy").unwrap().is_some());
    }
}
