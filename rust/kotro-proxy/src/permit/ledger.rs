//! One-shot permit ledger: unused → reserved → consumed (keyed by permit_digest).
//!
//! **No shared lockdir.** Each permit is a single claim file created with
//! `create_new`. A crashed process cannot leave a separate lock permanently
//! blocking the ledger — at worst the claim file itself remains in `reserved`
//! (intentional hold until `release_pre_agent`, `consume`, or operator delete).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermitLedgerState {
    Unused,
    Reserved,
    Consumed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerRecord {
    pub permit_digest: String,
    pub state: PermitLedgerState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_at: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("permit already reserved by another run")]
    AlreadyReserved,
    #[error("permit already consumed (replay rejected)")]
    AlreadyConsumed,
    #[error("permit not reserved by this run")]
    NotReserved,
    #[error("ledger I/O: {0}")]
    Io(String),
    #[error("ledger corrupt: {0}")]
    Corrupt(String),
}

/// File-backed ledger: `{root}/{digest_safe}.claim` — one file per permit.
pub struct PermitLedger {
    root: PathBuf,
}

impl PermitLedger {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| LedgerError::Io(e.to_string()))?;
        Ok(Self { root })
    }

    pub fn path_for(&self, permit_digest: &str) -> PathBuf {
        self.root
            .join(format!("{}.claim", digest_file_stem(permit_digest)))
    }

    /// Atomically unused → reserved via `create_new` on the claim file.
    /// Concurrent reserves: exactly one wins. No auxiliary lock directory.
    pub fn reserve(
        &self,
        permit_digest: &str,
        run_id: &str,
        now_rfc3339: &str,
    ) -> Result<(), LedgerError> {
        let path = self.path_for(permit_digest);
        let rec = LedgerRecord {
            permit_digest: permit_digest.to_string(),
            state: PermitLedgerState::Reserved,
            run_id: Some(run_id.to_string()),
            reserved_at: Some(now_rfc3339.to_string()),
            consumed_at: None,
        };
        match create_new_claim(&path, &rec) {
            Ok(()) => Ok(()),
            Err(LedgerError::AlreadyReserved) | Err(LedgerError::AlreadyConsumed) => {
                // Race: someone else created first — classify from content.
                let existing = read_record(&path)?;
                match existing.state {
                    PermitLedgerState::Consumed => Err(LedgerError::AlreadyConsumed),
                    PermitLedgerState::Reserved
                        if existing.run_id.as_deref() == Some(run_id) =>
                    {
                        Ok(())
                    }
                    PermitLedgerState::Reserved => Err(LedgerError::AlreadyReserved),
                    PermitLedgerState::Unused => {
                        // Corrupt / unexpected — treat as reserved conflict.
                        Err(LedgerError::AlreadyReserved)
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    /// reserved → consumed (agent started inside sandbox).
    pub fn consume(
        &self,
        permit_digest: &str,
        run_id: &str,
        now_rfc3339: &str,
    ) -> Result<(), LedgerError> {
        let path = self.path_for(permit_digest);
        let mut rec = self.load(permit_digest)?;
        match rec.state {
            PermitLedgerState::Reserved if rec.run_id.as_deref() == Some(run_id) => {
                rec.state = PermitLedgerState::Consumed;
                rec.consumed_at = Some(now_rfc3339.to_string());
                atomic_replace_claim(&path, &rec)
            }
            PermitLedgerState::Consumed => Err(LedgerError::AlreadyConsumed),
            _ => Err(LedgerError::NotReserved),
        }
    }

    /// reserved → unused on pre-agent failure (delete claim so next reserve can `create_new`).
    pub fn release_pre_agent(&self, permit_digest: &str, run_id: &str) -> Result<(), LedgerError> {
        let path = self.path_for(permit_digest);
        let rec = self.load(permit_digest)?;
        match rec.state {
            PermitLedgerState::Reserved if rec.run_id.as_deref() == Some(run_id) => {
                match fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(LedgerError::Io(e.to_string())),
                }
            }
            PermitLedgerState::Unused => Ok(()),
            PermitLedgerState::Consumed => Err(LedgerError::AlreadyConsumed),
            _ => Err(LedgerError::NotReserved),
        }
    }

    pub fn load(&self, permit_digest: &str) -> Result<LedgerRecord, LedgerError> {
        let path = self.path_for(permit_digest);
        if !path.exists() {
            return Ok(LedgerRecord {
                permit_digest: permit_digest.to_string(),
                state: PermitLedgerState::Unused,
                run_id: None,
                reserved_at: None,
                consumed_at: None,
            });
        }
        read_record(&path)
    }

    /// True when a claim file exists (reserved or consumed). Used by tests /
    /// R0.4 to assert verify-only did not claim.
    pub fn is_claimed(&self, permit_digest: &str) -> bool {
        self.path_for(permit_digest).exists()
    }
}

/// `O_CREAT|O_EXCL` equivalent — no shared lock file/dir to leak on crash.
fn create_new_claim(path: &Path, rec: &LedgerRecord) -> Result<(), LedgerError> {
    let data = serde_json::to_vec_pretty(rec).map_err(|e| LedgerError::Corrupt(e.to_string()))?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            f.write_all(&data)
                .map_err(|e| LedgerError::Io(e.to_string()))?;
            f.sync_all().map_err(|e| LedgerError::Io(e.to_string()))?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Caller classifies reserved vs consumed from file contents.
            Err(LedgerError::AlreadyReserved)
        }
        Err(e) => Err(LedgerError::Io(e.to_string())),
    }
}

fn atomic_replace_claim(path: &Path, rec: &LedgerRecord) -> Result<(), LedgerError> {
    let tmp = path.with_extension("claim.tmp");
    let data = serde_json::to_vec_pretty(rec).map_err(|e| LedgerError::Corrupt(e.to_string()))?;
    {
        let mut f = fs::File::create(&tmp).map_err(|e| LedgerError::Io(e.to_string()))?;
        f.write_all(&data).map_err(|e| LedgerError::Io(e.to_string()))?;
        f.sync_all().map_err(|e| LedgerError::Io(e.to_string()))?;
    }
    fs::rename(&tmp, path).map_err(|e| LedgerError::Io(e.to_string()))?;
    Ok(())
}

fn digest_file_stem(digest: &str) -> String {
    digest
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn read_record(path: &Path) -> Result<LedgerRecord, LedgerError> {
    let bytes = fs::read(path).map_err(|e| LedgerError::Io(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| LedgerError::Corrupt(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn reserve_consume_replay_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = PermitLedger::open(dir.path()).unwrap();
        let d = "sha256:abc";
        ledger.reserve(d, "run-1", "2026-08-01T18:00:00Z").unwrap();
        ledger.consume(d, "run-1", "2026-08-01T18:01:00Z").unwrap();
        assert_eq!(
            ledger
                .reserve(d, "run-2", "2026-08-01T18:02:00Z")
                .unwrap_err(),
            LedgerError::AlreadyConsumed
        );
    }

    #[test]
    fn release_pre_agent_allows_retry() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = PermitLedger::open(dir.path()).unwrap();
        let d = "sha256:def";
        ledger.reserve(d, "run-1", "2026-08-01T18:00:00Z").unwrap();
        ledger.release_pre_agent(d, "run-1").unwrap();
        assert!(!ledger.is_claimed(d));
        ledger.reserve(d, "run-2", "2026-08-01T18:03:00Z").unwrap();
        assert_eq!(ledger.load(d).unwrap().state, PermitLedgerState::Reserved);
    }

    #[test]
    fn concurrent_reserve_exactly_one_wins() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Arc::new(PermitLedger::open(dir.path()).unwrap());
        let d = "sha256:race";
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = vec![];
        for i in 0..8 {
            let ledger = Arc::clone(&ledger);
            let barrier = Arc::clone(&barrier);
            let run_id = format!("run-{i}");
            handles.push(thread::spawn(move || {
                barrier.wait();
                ledger.reserve(d, &run_id, "2026-08-01T18:00:00Z")
            }));
        }
        let mut wins = 0;
        for h in handles {
            if h.join().unwrap().is_ok() {
                wins += 1;
            }
        }
        assert_eq!(wins, 1);
        assert_eq!(ledger.load(d).unwrap().state, PermitLedgerState::Reserved);
    }

    #[test]
    fn crash_cannot_leave_sticky_lockdir() {
        // Invariant: ledger root must never require a `.lockdir` sidecar.
        let dir = tempfile::tempdir().unwrap();
        let ledger = PermitLedger::open(dir.path()).unwrap();
        let d = "sha256:nolock";
        ledger.reserve(d, "run-1", "2026-08-01T18:00:00Z").unwrap();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(entries.iter().any(|n| n.ends_with(".claim")));
        assert!(
            !entries.iter().any(|n| n.ends_with(".lockdir") || n.ends_with(".lock")),
            "must not create auxiliary lock sidecars: {entries:?}"
        );
    }
}
