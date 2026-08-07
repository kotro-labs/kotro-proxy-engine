//! Run-scoped token minting — `KOTRO_RUN_TOKEN` for the agent (never provider/GitHub keys).
//!
//! R3 attenuation: scopes + TTL bound into the HMAC record; one-shot land consume.

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
    #[error("token expired")]
    Expired,
    #[error("token scope denied")]
    ScopeDenied,
    #[error("token already consumed for land")]
    LandConsumed,
}

#[derive(Debug, Clone)]
pub struct RunToken {
    /// Opaque bearer value injected into the agent.
    pub token: String,
    pub run_id: String,
    pub permit_digest: String,
    pub scopes: Vec<String>,
    pub expires_at: String,
    /// Host-side record path (never mounted into agent).
    pub record_path: PathBuf,
}

/// Mint a run token bound to `permit_digest` + `run_id` with default attenuation
/// (`draft_pr` scope). Prefer [`mint_run_token_attenuated`] with the envelope
/// `expires_at` from `run --permit`.
pub fn mint_run_token(
    ledger_dir: &Path,
    permit_digest: &str,
    run_id: &str,
) -> Result<RunToken, TokenError> {
    mint_run_token_attenuated(
        ledger_dir,
        permit_digest,
        run_id,
        &["draft_pr"],
        "2099-01-01T00:00:00Z",
    )
}

/// Mint with explicit scopes and expiry (R3).
pub fn mint_run_token_attenuated(
    ledger_dir: &Path,
    permit_digest: &str,
    run_id: &str,
    scopes: &[&str],
    expires_at: &str,
) -> Result<RunToken, TokenError> {
    fs::create_dir_all(ledger_dir).map_err(|e| TokenError::Io(e.to_string()))?;
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = b64url(&raw);
    let scopes_owned: Vec<String> = scopes.iter().map(|s| (*s).to_string()).collect();

    let key_path = ledger_dir.join("run-token-hmac.key");
    let key = load_or_create_key(&key_path)?;
    let tag = hmac_tag(
        &key,
        permit_digest,
        run_id,
        &scopes_owned,
        expires_at,
        &token,
    )?;

    let stem = digest_stem(permit_digest);
    let record_path = ledger_dir.join(format!("{stem}.{run_id}.run-token.json"));
    let body = serde_json::json!({
        "permit_digest": permit_digest,
        "run_id": run_id,
        "scopes": scopes_owned,
        "expires_at": expires_at,
        "token_hmac_sha256": hex::encode(tag),
        "land_consumed": false,
    });
    atomic_write(&record_path, body.to_string().as_bytes())?;

    Ok(RunToken {
        token,
        run_id: run_id.to_string(),
        permit_digest: permit_digest.to_string(),
        scopes: scopes_owned,
        expires_at: expires_at.to_string(),
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
    verify_run_token_for_scope(ledger_dir, permit_digest, run_id, presented, "draft_pr")
}

/// Verify + require a scope; reject expired or land-consumed tokens.
pub fn verify_run_token_for_scope(
    ledger_dir: &Path,
    permit_digest: &str,
    run_id: &str,
    presented: &str,
    required_scope: &str,
) -> Result<bool, TokenError> {
    let key_path = ledger_dir.join("run-token-hmac.key");
    let key = fs::read(&key_path).map_err(|e| TokenError::Io(e.to_string()))?;
    let stem = digest_stem(permit_digest);
    let record_path = ledger_dir.join(format!("{stem}.{run_id}.run-token.json"));
    let raw = fs::read(&record_path).map_err(|e| TokenError::Io(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| TokenError::Io(e.to_string()))?;

    if v.get("land_consumed").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Err(TokenError::LandConsumed);
    }

    let expires_at = v
        .get("expires_at")
        .and_then(|x| x.as_str())
        .unwrap_or("2099-01-01T00:00:00Z");
    let now = crate::flight_recorder::now_rfc3339();
    if let (Ok(now_t), Ok(exp_t)) = (
        kotro_types::parse_rfc3339(&now),
        kotro_types::parse_rfc3339(expires_at),
    ) {
        if now_t >= exp_t {
            return Err(TokenError::Expired);
        }
    }

    let scopes: Vec<String> = v
        .get("scopes")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| vec!["draft_pr".into()]);
    if !scopes.iter().any(|s| s == required_scope) {
        return Err(TokenError::ScopeDenied);
    }
    // Never allow merge via run token.
    if scopes.iter().any(|s| s == "merge") {
        return Err(TokenError::ScopeDenied);
    }

    let expected = v
        .get("token_hmac_sha256")
        .and_then(|x| x.as_str())
        .ok_or_else(|| TokenError::Io("missing hmac".into()))?;

    let tag = hmac_tag(&key, permit_digest, run_id, &scopes, expires_at, presented)?;
    Ok(constant_time_eq(
        hex::encode(tag).as_bytes(),
        expected.as_bytes(),
    ))
}

/// Mark the run token as consumed for land (one successful draft-pr).
pub fn consume_run_token_for_land(
    ledger_dir: &Path,
    permit_digest: &str,
    run_id: &str,
) -> Result<(), TokenError> {
    let stem = digest_stem(permit_digest);
    let record_path = ledger_dir.join(format!("{stem}.{run_id}.run-token.json"));
    let raw = fs::read(&record_path).map_err(|e| TokenError::Io(e.to_string()))?;
    let mut v: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| TokenError::Io(e.to_string()))?;
    if v.get("land_consumed").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Err(TokenError::LandConsumed);
    }
    v["land_consumed"] = serde_json::json!(true);
    atomic_write(
        &record_path,
        serde_json::to_string(&v)
            .map_err(|e| TokenError::Io(e.to_string()))?
            .as_bytes(),
    )?;
    Ok(())
}

fn hmac_tag(
    key: &[u8],
    permit_digest: &str,
    run_id: &str,
    scopes: &[String],
    expires_at: &str,
    token: &str,
) -> Result<[u8; 32], TokenError> {
    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| TokenError::Io(e.to_string()))?;
    mac.update(permit_digest.as_bytes());
    mac.update(b"\0");
    mac.update(run_id.as_bytes());
    mac.update(b"\0");
    let mut scopes_sorted = scopes.to_vec();
    scopes_sorted.sort();
    mac.update(scopes_sorted.join(",").as_bytes());
    mac.update(b"\0");
    mac.update(expires_at.as_bytes());
    mac.update(b"\0");
    mac.update(token.as_bytes());
    let tag = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&tag);
    Ok(out)
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
        let t = mint_run_token_attenuated(
            dir.path(),
            "sha256:abc",
            "run-1",
            &["draft_pr"],
            "2099-01-01T00:00:00Z",
        )
        .unwrap();
        assert!(verify_run_token(dir.path(), "sha256:abc", "run-1", &t.token).unwrap());
        assert!(!verify_run_token(dir.path(), "sha256:abc", "run-1", "wrong").unwrap());
        let rec = fs::read_to_string(&t.record_path).unwrap();
        assert!(!rec.contains(&t.token));
        assert!(rec.contains("draft_pr"));
    }

    #[test]
    fn expired_and_consumed_and_scope() {
        let dir = tempfile::tempdir().unwrap();
        let t = mint_run_token_attenuated(
            dir.path(),
            "sha256:e",
            "run-e",
            &["draft_pr"],
            "2020-01-01T00:00:00Z",
        )
        .unwrap();
        assert!(matches!(
            verify_run_token(dir.path(), "sha256:e", "run-e", &t.token),
            Err(TokenError::Expired)
        ));

        let t2 = mint_run_token_attenuated(
            dir.path(),
            "sha256:c",
            "run-c",
            &["draft_pr"],
            "2099-01-01T00:00:00Z",
        )
        .unwrap();
        consume_run_token_for_land(dir.path(), "sha256:c", "run-c").unwrap();
        assert!(matches!(
            verify_run_token(dir.path(), "sha256:c", "run-c", &t2.token),
            Err(TokenError::LandConsumed)
        ));
    }
}
