//! Signed land receipts (R3) — mediator-signed evidence of a brokered land action.
//!
//! Separate from permit-authority keys: the **receipt mediator** signs receipts;
//! operators trust mediator pubkeys via `--trust` (issuer `kotro://mediator/land-receipt`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use kotro_types::{key_id_for_public_key, public_key_b64, TrustStore, API_VERSION_V1ALPHA1};

/// Distinct from TaskEnvelope signing domains.
pub const LAND_RECEIPT_SIGNING_DOMAIN: &[u8] = b"kotro-land-receipt-v1\n";
pub const LAND_RECEIPT_KIND: &str = "LandReceipt";
pub const LAND_RECEIPT_MEDIATOR: &str = "kotro://mediator/land-receipt";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LandReceiptSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LandReceipt {
    pub api_version: String,
    pub kind: String,
    pub permit_digest: String,
    pub run_id: String,
    pub land_action: String,
    pub repository_identity: String,
    pub base_ref: String,
    pub base_sha: String,
    pub head_branch: String,
    pub artifact_hash: String,
    pub pr_url: String,
    pub draft: bool,
    pub issued_at: String,
    pub mediator: String,
    pub signature: LandReceiptSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptVerifyLevels {
    pub signature_valid: bool,
    pub signer_trusted: bool,
    pub permit_digest_bound: bool,
    pub chain_complete: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReceiptVerifyError {
    #[error("trust store missing: {0}")]
    TrustMissing(String),
    #[error("receipt file missing: {0}")]
    ReceiptMissing(String),
    #[error("receipt malformed: {0}")]
    Malformed(String),
    #[error(
        "receipt verify failed: signature_valid={signature_valid} signer_trusted={signer_trusted} permit_digest_bound={permit_digest_bound} chain_complete={chain_complete}"
    )]
    Failed {
        signature_valid: bool,
        signer_trusted: bool,
        permit_digest_bound: bool,
        chain_complete: bool,
    },
}

#[derive(Debug, Error)]
pub enum ReceiptIssueError {
    #[error("receipt I/O: {0}")]
    Io(String),
    #[error("receipt sign: {0}")]
    Sign(String),
}

/// Fail-closed stub retained for migration — delegates to real verify.
pub fn verify_receipt_stub(receipt: &Path, trust: &Path) -> Result<(), ReceiptVerifyError> {
    verify_land_receipt(receipt, trust).map(|_| ())
}

/// Verify a land receipt against a trust store.
///
/// Levels (AUTHORITY R3):
/// - **signature_valid** — Ed25519 over domain + JCS body
/// - **signer_trusted** — key_id active in trust for `kotro://mediator/land-receipt`
/// - **permit_digest_bound** — non-empty `permit_digest` present
/// - **chain_complete** — all of the above
pub fn verify_land_receipt(
    receipt_path: &Path,
    trust_path: &Path,
) -> Result<ReceiptVerifyLevels, ReceiptVerifyError> {
    if !trust_path.exists() {
        return Err(ReceiptVerifyError::TrustMissing(
            trust_path.display().to_string(),
        ));
    }
    if !receipt_path.exists() {
        return Err(ReceiptVerifyError::ReceiptMissing(
            receipt_path.display().to_string(),
        ));
    }
    let raw = fs::read(receipt_path).map_err(|e| ReceiptVerifyError::Malformed(e.to_string()))?;
    let receipt: LandReceipt =
        serde_json::from_slice(&raw).map_err(|e| ReceiptVerifyError::Malformed(e.to_string()))?;
    if receipt.kind != LAND_RECEIPT_KIND {
        return Err(ReceiptVerifyError::Malformed(format!(
            "kind={}",
            receipt.kind
        )));
    }

    let permit_digest_bound =
        !receipt.permit_digest.is_empty() && receipt.permit_digest.starts_with("sha256:");

    let now = crate::flight_recorder::now_rfc3339();
    let trust = TrustStore::load(trust_path).map_err(|_| {
        ReceiptVerifyError::TrustMissing(trust_path.display().to_string())
    })?;

    let signer_trusted = trust
        .find_active(&receipt.signature.key_id, LAND_RECEIPT_MEDIATOR, &now)
        .is_ok();

    // Cryptographic check: use matching key from store (trusted or not) if present.
    let signature_valid = trust
        .keys
        .iter()
        .find(|k| k.key_id == receipt.signature.key_id)
        .and_then(|tk| decode_verifying_key(&tk.public_key).ok())
        .map(|vk| verify_signature_with_pubkey(&receipt, &vk).unwrap_or(false))
        .unwrap_or(false);

    let chain_complete = signature_valid && signer_trusted && permit_digest_bound;
    let levels = ReceiptVerifyLevels {
        signature_valid,
        signer_trusted,
        permit_digest_bound,
        chain_complete,
    };
    if !chain_complete {
        return Err(ReceiptVerifyError::Failed {
            signature_valid,
            signer_trusted,
            permit_digest_bound,
            chain_complete,
        });
    }
    Ok(levels)
}

/// Issue a signed land receipt under `ledger_dir` and return its path.
pub fn issue_land_receipt(
    ledger_dir: &Path,
    receipt: &mut LandReceipt,
    signing_key: &SigningKey,
) -> Result<PathBuf, ReceiptIssueError> {
    sign_land_receipt(receipt, signing_key).map_err(ReceiptIssueError::Sign)?;
    fs::create_dir_all(ledger_dir).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
    let path = ledger_dir.join(format!(
        "{}.land-receipt.json",
        receipt.run_id.replace('/', "_")
    ));
    let body =
        serde_json::to_vec_pretty(receipt).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
    atomic_write(&path, &body)?;
    Ok(path)
}

/// Load or create a host-only mediator signing key under the ledger dir.
pub fn load_or_create_mediator_key(ledger_dir: &Path) -> Result<SigningKey, ReceiptIssueError> {
    fs::create_dir_all(ledger_dir).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
    let path = ledger_dir.join("receipt-mediator.ed25519");
    if path.exists() {
        let bytes = fs::read(&path).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(ReceiptIssueError::Io(
                "receipt-mediator.ed25519 must be 32 bytes".into(),
            ));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        return Ok(SigningKey::from_bytes(&seed));
    }
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    atomic_write(&path, sk.to_bytes().as_slice())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)
            .map_err(|e| ReceiptIssueError::Io(e.to_string()))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
    }
    let vk = sk.verifying_key();
    let pub_path = ledger_dir.join("receipt-mediator.pub.json");
    let pub_body = serde_json::json!({
        "key_id": key_id_for_public_key(vk.as_bytes()),
        "algorithm": "Ed25519",
        "public_key": public_key_b64(&vk),
        "issuer": LAND_RECEIPT_MEDIATOR,
        "note": "Add this TrustKey to --trust with issuers including kotro://mediator/land-receipt",
    });
    atomic_write(
        &pub_path,
        serde_json::to_string_pretty(&pub_body)
            .map_err(|e| ReceiptIssueError::Io(e.to_string()))?
            .as_bytes(),
    )?;
    Ok(sk)
}

#[allow(clippy::too_many_arguments)]
pub fn build_draft_pr_receipt(
    permit_digest: &str,
    run_id: &str,
    repository_identity: &str,
    base_ref: &str,
    base_sha: &str,
    head_branch: &str,
    artifact_hash: &str,
    pr_url: &str,
) -> LandReceipt {
    LandReceipt {
        api_version: API_VERSION_V1ALPHA1.into(),
        kind: LAND_RECEIPT_KIND.into(),
        permit_digest: permit_digest.into(),
        run_id: run_id.into(),
        land_action: "draft_pr".into(),
        repository_identity: repository_identity.into(),
        base_ref: base_ref.into(),
        base_sha: base_sha.into(),
        head_branch: head_branch.into(),
        artifact_hash: artifact_hash.into(),
        pr_url: pr_url.into(),
        draft: true,
        issued_at: crate::flight_recorder::now_rfc3339(),
        mediator: LAND_RECEIPT_MEDIATOR.into(),
        signature: LandReceiptSignature {
            algorithm: "Ed25519".into(),
            key_id: String::new(),
            value: String::new(),
        },
    }
}

fn sign_land_receipt(receipt: &mut LandReceipt, sk: &SigningKey) -> Result<(), String> {
    let vk = sk.verifying_key();
    receipt.signature.key_id = key_id_for_public_key(vk.as_bytes());
    receipt.signature.algorithm = "Ed25519".into();
    let msg = signing_input(receipt)?;
    let sig = sk.sign(&msg);
    receipt.signature.value =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes());
    Ok(())
}

fn verify_signature_with_pubkey(receipt: &LandReceipt, vk: &VerifyingKey) -> Result<bool, String> {
    let msg = signing_input(receipt)?;
    let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(receipt.signature.value.as_bytes())
        .map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    Ok(vk.verify(&msg, &sig).is_ok())
}

fn signing_input(receipt: &LandReceipt) -> Result<Vec<u8>, String> {
    let mut v = serde_json::to_value(receipt).map_err(|e| e.to_string())?;
    if let Some(obj) = v.as_object_mut() {
        obj.remove("signature");
    }
    let jcs = serde_json_canonicalizer::to_vec(&v).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(LAND_RECEIPT_SIGNING_DOMAIN.len() + jcs.len());
    out.extend_from_slice(LAND_RECEIPT_SIGNING_DOMAIN);
    out.extend_from_slice(&jcs);
    Ok(out)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), ReceiptIssueError> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
        f.write_all(data)
            .map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
        f.sync_all()
            .map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
    }
    fs::rename(&tmp, path).map_err(|e| ReceiptIssueError::Io(e.to_string()))?;
    Ok(())
}

fn decode_verifying_key(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64.as_bytes())
        .map_err(|e| e.to_string())?;
    VerifyingKey::from_bytes(
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| "pubkey len".to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kotro_types::TrustKey;
    use std::os::unix::fs::PermissionsExt;

    fn trust_with(sk: &SigningKey, trusted: bool) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let vk = sk.verifying_key();
        let path = dir.path().join("trust.json");
        let store = TrustStore {
            keys: vec![TrustKey {
                key_id: key_id_for_public_key(vk.as_bytes()),
                algorithm: "Ed25519".into(),
                public_key: public_key_b64(&vk),
                issuers: if trusted {
                    vec![LAND_RECEIPT_MEDIATOR.into()]
                } else {
                    vec!["kotro://authority/other".into()]
                },
                status: "active".into(),
                not_before: "2020-01-01T00:00:00Z".into(),
                not_after: "2099-01-01T00:00:00Z".into(),
            }],
            revoked_key_ids: vec![],
            revoked_task_ids: vec![],
            revoked_envelope_digests: vec![],
        };
        let body = serde_json::to_vec_pretty(&store).unwrap();
        fs::write(&path, &body).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms).unwrap();
        (dir, path)
    }

    #[test]
    fn issue_and_verify_chain_complete() {
        let sk = SigningKey::generate(&mut rand::rngs::OsRng);
        let (_td, trust) = trust_with(&sk, true);
        let ledger = tempfile::tempdir().unwrap();
        let mut r = build_draft_pr_receipt(
            "sha256:abcd",
            "run-1",
            "github.com/o/r",
            "main",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "kotro/run-1",
            "sha256:artifact",
            "https://example.invalid/o/r/pull/1",
        );
        let path = issue_land_receipt(ledger.path(), &mut r, &sk).unwrap();
        let levels = verify_land_receipt(&path, &trust).unwrap();
        assert!(levels.chain_complete);
        assert!(levels.signature_valid);
        assert!(levels.signer_trusted);
        assert!(levels.permit_digest_bound);
    }

    #[test]
    fn attacker_receipt_signature_valid_but_untrusted() {
        // Suite #19 — attacker-signed receipt must not be chain_complete.
        let sk = SigningKey::generate(&mut rand::rngs::OsRng);
        let (_td, trust) = trust_with(&sk, false);
        let ledger = tempfile::tempdir().unwrap();
        let mut r = build_draft_pr_receipt(
            "sha256:abcd",
            "run-atk",
            "github.com/o/r",
            "main",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "kotro/run-atk",
            "sha256:artifact",
            "https://evil.example/pull/9",
        );
        let path = issue_land_receipt(ledger.path(), &mut r, &sk).unwrap();
        let err = verify_land_receipt(&path, &trust).unwrap_err();
        match err {
            ReceiptVerifyError::Failed {
                signature_valid,
                signer_trusted,
                chain_complete,
                ..
            } => {
                assert!(signature_valid);
                assert!(!signer_trusted);
                assert!(!chain_complete);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tampered_receipt_fails_signature() {
        let sk = SigningKey::generate(&mut rand::rngs::OsRng);
        let (_td, trust) = trust_with(&sk, true);
        let ledger = tempfile::tempdir().unwrap();
        let mut r = build_draft_pr_receipt(
            "sha256:abcd",
            "run-2",
            "github.com/o/r",
            "main",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "kotro/run-2",
            "sha256:artifact",
            "https://example.invalid/pull/2",
        );
        let path = issue_land_receipt(ledger.path(), &mut r, &sk).unwrap();
        let mut raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        raw["pr_url"] = serde_json::json!("https://evil.example/pull/99");
        fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let err = verify_land_receipt(&path, &trust).unwrap_err();
        match err {
            ReceiptVerifyError::Failed {
                signature_valid,
                chain_complete,
                ..
            } => {
                assert!(!signature_valid);
                assert!(!chain_complete);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
