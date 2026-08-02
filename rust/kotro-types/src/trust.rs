//! Operator-owned trust store for TaskEnvelope root keys.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::reason::TaskReason;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustStore {
    pub keys: Vec<TrustKey>,
    #[serde(default)]
    pub revoked_key_ids: Vec<String>,
    #[serde(default)]
    pub revoked_task_ids: Vec<String>,
    #[serde(default)]
    pub revoked_envelope_digests: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustKey {
    pub key_id: String,
    pub algorithm: String,
    pub public_key: String,
    pub issuers: Vec<String>,
    pub status: String,
    pub not_before: String,
    pub not_after: String,
}

impl TrustStore {
    pub fn load(path: &Path) -> Result<Self, TaskReason> {
        let meta = fs::metadata(path).map_err(|_| TaskReason::TaskKeyUntrusted)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode();
            // Reject group/world writable trust stores (bits 0022).
            if mode & 0o022 != 0 {
                return Err(TaskReason::TaskKeyUntrusted);
            }
        }
        let bytes = fs::read(path).map_err(|_| TaskReason::TaskKeyUntrusted)?;
        // Never interpret key_id as a filesystem path — load only this registry.
        serde_json::from_slice(&bytes).map_err(|_| TaskReason::TaskMalformed)
    }

    pub fn find_active(&self, key_id: &str, issuer: &str, now: &str) -> Result<&TrustKey, TaskReason> {
        if self.revoked_key_ids.iter().any(|k| k == key_id) {
            return Err(TaskReason::TaskKeyRevoked);
        }
        let key = self
            .keys
            .iter()
            .find(|k| k.key_id == key_id)
            .ok_or(TaskReason::TaskKeyUntrusted)?;
        if key.algorithm != "Ed25519" || key.status != "active" {
            return Err(TaskReason::TaskKeyUntrusted);
        }
        if !key.issuers.iter().any(|i| i == issuer) {
            return Err(TaskReason::TaskKeyUntrusted);
        }
        if now < key.not_before.as_str() || now > key.not_after.as_str() {
            return Err(TaskReason::TaskKeyUntrusted);
        }
        Ok(key)
    }

    pub fn is_revoked_task(&self, task_id: &str) -> bool {
        self.revoked_task_ids.iter().any(|t| t == task_id)
    }

    pub fn is_revoked_digest(&self, digest: &str) -> bool {
        self.revoked_envelope_digests.iter().any(|d| d == digest)
    }
}

/// Local content-addressed parent store. No network access.
pub trait ParentStore {
    fn get(&self, digest: &str) -> Option<crate::envelope::TaskEnvelope>;
}

#[derive(Default)]
pub struct MemoryParentStore {
    pub entries: std::collections::HashMap<String, crate::envelope::TaskEnvelope>,
}

impl ParentStore for MemoryParentStore {
    fn get(&self, digest: &str) -> Option<crate::envelope::TaskEnvelope> {
        self.entries.get(digest).cloned()
    }
}
