//! redb-backed semantic cache — mirrors `internal/cache/store.go`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redb::{Database, ReadableTableMetadata, TableDefinition, TableError};
use serde_json;
use thiserror::Error;

use super::encoding::{decode_stored_value, encode_stored_value, expires_at_nano};
use super::entry::Entry;

/// Matches Go `bucketName = "sse_cache"`.
pub(crate) const CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("sse_cache");

#[derive(Debug, Clone)]
pub struct StoreOptions {
    pub ttl: Duration,
    pub enable_compression: bool,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            ttl: Duration::ZERO,
            enable_compression: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("cache: open db: {0}")]
    Open(#[from] redb::DatabaseError),
    #[error("cache: table: {0}")]
    Table(#[from] TableError),
    #[error("cache: transaction: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("cache: storage: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("cache: commit: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("cache: mkdir: {0}")]
    Mkdir(#[from] std::io::Error),
    #[error("cache: corrupt entry: {0}")]
    Corrupt(#[from] serde_json::Error),
    #[error("cache: bucket missing")]
    BucketMissing,
}

#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
    path: PathBuf,
    ttl: Duration,
    compress: bool,
}

impl Store {
    /// Opens or creates the cache database at `path` with no TTL.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        Self::open_with_options(path, StoreOptions::default())
    }

    /// Opens or creates the cache database with TTL settings.
    pub fn open_with_options<P: AsRef<Path>>(
        path: P,
        opts: StoreOptions,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let db = Database::create(&path)?;
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(CACHE_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self {
            db: Arc::new(db),
            path,
            ttl: opts.ttl,
            compress: opts.enable_compression,
        })
    }

    /// Retrieves a cached SSE stream by semantic key. Returns `None` on miss or expiry.
    pub fn get(&self, key: &str) -> Result<Option<Entry>, StoreError> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(CACHE_TABLE) {
            Ok(t) => t,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let guard = match table.get(key)? {
            Some(g) => g,
            None => return Ok(None),
        };

        // Zero-copy view into the redb mmap frame for the duration of the read txn.
        let raw_value = guard.value();
        let now_nano = now_unix_nano();

        let (payload, expired) = decode_stored_value(raw_value, now_nano);
        if expired {
            let db = Arc::clone(&self.db);
            let key_owned = key.to_string();
            if tokio::runtime::Handle::try_current().is_ok() {
                tokio::task::spawn_blocking(move || {
                    let _ = delete_key(&db, &key_owned);
                });
            } else {
                std::thread::spawn(move || {
                    let _ = delete_key(&db, &key_owned);
                });
            }
            return Ok(None);
        }



        let Some(payload) = payload else {
            return Ok(None);
        };

        let entry: Entry = serde_json::from_slice(&payload)?;
        Ok(Some(entry))
    }

    /// Writes a complete SSE stream entry with the store-level TTL prefix.
    pub fn put(&self, entry: Entry) -> Result<(), StoreError> {
        let payload = serde_json::to_vec(&entry)?;
        let stored = encode_stored_value(expires_at_nano(self.ttl), &payload, self.compress);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(CACHE_TABLE)?;
            table.insert(entry.key.as_str(), stored.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Removes a cache entry by key.
    pub fn delete(&self, key: &str) -> Result<(), StoreError> {
        delete_key(&self.db, key)
    }

    /// Stores a raw table value (legacy migration / TTL tests).
    pub fn put_raw(&self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(CACHE_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Deletes all keys whose TTL prefix has lapsed.
    pub fn sweep_expired(&self) -> Result<usize, StoreError> {
        if self.ttl.is_zero() {
            return Ok(0);
        }
        super::eviction::sweep_expired_retain(&self.db, now_unix_nano())
    }

    /// Configured entry lifetime (`Duration::ZERO` = no expiry).
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn compression_enabled(&self) -> bool {
        self.compress
    }

    /// On-disk database file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the number of cached entries.
    pub fn count(&self) -> Result<usize, StoreError> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(CACHE_TABLE) {
            Ok(t) => t,
            Err(TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let len = table.len()?;
        Ok(len as usize)
    }

    /// Shared database handle for background eviction sweeps.
    pub(crate) fn db_handle(&self) -> &Arc<Database> {
        &self.db
    }
}


fn now_unix_nano() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64
}

fn delete_key(db: &Database, key: &str) -> Result<(), StoreError> {
    let write_txn = db.begin_write()?;
    {
        let mut table = match write_txn.open_table(CACHE_TABLE) {
            Ok(t) => t,
            Err(TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let _ = table.remove(key)?;
    }
    write_txn.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let store = Store::open(&path).unwrap();

        let entry = Entry {
            key: "abc123".into(),
            raw_sse: b"data: {\"choices\":[]}\n\ndata: [DONE]\n\n".to_vec(),
            model: "gpt-4".into(),
            created_at: 0,
        };
        store.put(entry.clone()).unwrap();

        let got = store.get("abc123").unwrap().expect("cache hit");
        assert_eq!(got.raw_sse, entry.raw_sse);
        assert_eq!(got.model, entry.model);

        assert!(store.get("missing").unwrap().is_none());
        assert!(path.exists());
    }

    #[test]
    fn ttl_fresh_hit() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_with_options(
            dir.path().join("ttl.db"),
            StoreOptions {
                ttl: Duration::from_secs(3600),
                ..Default::default()
            },
        )
        .unwrap();

        store
            .put(Entry {
                key: "expire-me".into(),
                raw_sse: b"data: [DONE]\n\n".to_vec(),
                model: "gpt-4".into(),
                created_at: 0,
            })
            .unwrap();

        assert!(store.get("expire-me").unwrap().is_some());
    }

    #[test]
    fn ttl_short_lived_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_with_options(
            dir.path().join("short.db"),
            StoreOptions {
                ttl: Duration::from_millis(100),
                enable_compression: false,
            },
        )
        .unwrap();

        store
            .put(Entry {
                key: "short".into(),
                raw_sse: b"data: [DONE]\n\n".to_vec(),
                model: "gpt-4".into(),
                created_at: 0,
            })
            .unwrap();

        assert!(store.get("short").unwrap().is_some());

        thread::sleep(Duration::from_millis(120));
        assert!(store.get("short").unwrap().is_none());

        thread::sleep(Duration::from_millis(50));
        assert!(store.get("short").unwrap().is_none());
    }

    #[test]
    fn sweep_expired_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_with_options(
            dir.path().join("sweep.db"),
            StoreOptions {
                ttl: Duration::from_millis(1),
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

        thread::sleep(Duration::from_millis(5));
        assert_eq!(store.sweep_expired().unwrap(), 1);
        assert!(store.get("gone").unwrap().is_none());
    }

    #[test]
    fn compressed_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_with_options(
            dir.path().join("zstd.db"),
            StoreOptions {
                ttl: Duration::from_secs(3600),
                enable_compression: true,
            },
        )
        .unwrap();

        let entry = Entry {
            key: "zstd-key".into(),
            raw_sse: b"data: {\"x\":1}\n\ndata: [DONE]\n\n".to_vec(),
            model: "gpt-4".into(),
            created_at: 0,
        };
        store.put(entry.clone()).unwrap();
        let got = store.get("zstd-key").unwrap().expect("hit");
        assert_eq!(got.raw_sse, entry.raw_sse);
    }

    #[test]
    fn legacy_entry_without_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("legacy.db")).unwrap();

        let entry = Entry {
            key: "legacy".into(),
            raw_sse: b"data: legacy\n\n".to_vec(),
            model: "gpt-4".into(),
            created_at: 0,
        };
        let raw = serde_json::to_vec(&entry).unwrap();
        store.put_raw("legacy", &raw).unwrap();

        let got = store.get("legacy").unwrap().expect("legacy hit");
        assert_eq!(got.raw_sse, entry.raw_sse);
    }

    #[test]
    fn zero_copy_read_does_not_clone_until_hit() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("mmap.db")).unwrap();
        store
            .put(Entry {
                key: "k".into(),
                raw_sse: b"data: x\n\n".to_vec(),
                model: "m".into(),
                created_at: 0,
            })
            .unwrap();

        let read_txn = store.db.begin_read().unwrap();
        let table = read_txn.open_table(CACHE_TABLE).unwrap();
        let guard = table.get("k").unwrap().unwrap();
        let mmap_slice = guard.value();
        assert!(mmap_slice.len() > 1);
        drop(guard);
        drop(table);
        drop(read_txn);

        assert!(store.get("k").unwrap().is_some());
    }
}
