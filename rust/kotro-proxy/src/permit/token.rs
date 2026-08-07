//! Run-scoped token minting — `KOTRO_RUN_TOKEN` for the agent (never provider/GitHub keys).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("token I/O: {0}")]
    Io(String),
}

#[derive(Debug, Clone)]
pub struct RunToken {
    /// Opaque bearer value injected into the agent.
    pub token: String,
    pub run_id: String,
    pub permit_digest: String,
    /// Host-side record path (never mounted into agent).
    pub record_path: PathBuf,
}

/// Mint a run token bound to `permit_digest` + `run_id`.
///
/// The raw token enters the agent; the HMAC of the token is stored host-side
/// for later broker validation (R2-B). Provider/GitHub secrets must not be
/// derived from or stored alongside this record.
pub fn mint_run_token(
    ledger_dir: &Path,
    permit_digest: &str,
    run_id: &str,
) -> Result<RunToken, TokenError> {
    fs::create_dir_all(ledger_dir).map_err(|e| TokenError::Io(e.to_string()))?;
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = b64url(&raw);

    // Per-install minting key material (ephemeral file under ledger dir).
    let key_path = ledger_dir.join("run-token-hmac.key");
    let key = load_or_create_key(&key_path)?;
    let mut mac =
        HmacSha256::new_from_slice(&key).map_err(|e| TokenError::Io(e.to_string()))?;
    mac.update(permit_digest.as_bytes());
    mac.update(b"\0");
    mac.update(run_id.as_bytes());
    mac.update(b"\0");
    mac.update(token.as_bytes());
    let tag = mac.finalize().into_bytes();

    let stem = digest_stem(permit_digest);
    let record_path = ledger_dir.join(format!("{stem}.{run_id}.run-token.json"));
    let body = serde_json::json!({
        "permit_digest": permit_digest,
        "run_id": run_id,
        "token_hmac_sha256": hex::encode(tag),
        // Never store the raw token on disk in plaintext long-term — omit it.
    });
    atomic_write(&record_path, body.to_string().as_bytes())?;

    Ok(RunToken {
        token,
        run_id: run_id.to_string(),
        permit_digest: permit_digest.to_string(),
        record_path,
    })
}

/// Verify a presented bearer against the host-side HMAC record.
pub fn verify_run_token(
    ledger_dir: &Path,
    permit_digest: &str,
    run_id: &str,
    presented: &str,
) -> Result<bool, TokenError> {
    let key_path = ledger_dir.join("run-token-hmac.key");
    let key = fs::read(&key_path).map_err(|e| TokenError::Io(e.to_string()))?;
    let stem = digest_stem(permit_digest);
    let record_path = ledger_dir.join(format!("{stem}.{run_id}.run-token.json"));
    let raw = fs::read(&record_path).map_err(|e| TokenError::Io(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| TokenError::Io(e.to_string()))?;
    let expected = v
        .get("token_hmac_sha256")
        .and_then(|x| x.as_str())
        .ok_or_else(|| TokenError::Io("missing hmac".into()))?;

    let mut mac =
        HmacSha256::new_from_slice(&key).map_err(|e| TokenError::Io(e.to_string()))?;
    mac.update(permit_digest.as_bytes());
    mac.update(b"\0");
    mac.update(run_id.as_bytes());
    mac.update(b"\0");
    mac.update(presented.as_bytes());
    let tag = hex::encode(mac.finalize().into_bytes());
    Ok(constant_time_eq(tag.as_bytes(), expected.as_bytes()))
}

fn load_or_create_key(path: &Path) -> Result<Vec<u8>, TokenError> {
    if path.exists() {
        return fs::read(path).map_err(|e| TokenError::Io(e.to_string()));
    }
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    atomic_write(path, &key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|e| TokenError::Io(e.to_string()))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| TokenError::Io(e.to_string()))?;
    }
    Ok(key.to_vec())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), TokenError> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| TokenError::Io(e.to_string()))?;
        f.write_all(data).map_err(|e| TokenError::Io(e.to_string()))?;
        f.sync_all().map_err(|e| TokenError::Io(e.to_string()))?;
    }
    fs::rename(&tmp, path).map_err(|e| TokenError::Io(e.to_string()))?;
    Ok(())
}

fn digest_stem(digest: &str) -> String {
    digest
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(80)
        .collect()
}

fn b64url(bytes: &[u8]) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        const HEX: &[u8] = b"0123456789abcdef";
        let mut s = String::with_capacity(bytes.as_ref().len() * 2);
        for &b in bytes.as_ref() {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0xf) as usize] as char);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_and_verify_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let t = mint_run_token(dir.path(), "sha256:abc", "run-1").unwrap();
        assert!(verify_run_token(dir.path(), "sha256:abc", "run-1", &t.token).unwrap());
        assert!(!verify_run_token(dir.path(), "sha256:abc", "run-1", "wrong").unwrap());
        // Raw token must not appear in the host record.
        let rec = fs::read_to_string(&t.record_path).unwrap();
        assert!(!rec.contains(&t.token));
    }
}
