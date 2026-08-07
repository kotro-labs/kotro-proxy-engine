//! Thin draft-PR broker (R2-B) — host-owned clean git + allow-once + run token.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::permit::token::verify_run_token;
use kotro_types::{parse_envelope_bytes, LandMode, TaskEnvelope};

/// Session written by `run --permit` when `land.mode=draft_pr`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerSession {
    pub permit_digest: String,
    pub run_id: String,
    pub ledger_dir: PathBuf,
    pub permit_path: PathBuf,
    pub live_repo: PathBuf,
    pub staged_dir: PathBuf,
    pub review_diff: PathBuf,
    pub repository_identity: String,
    pub base_ref: String,
    pub base_sha: String,
    pub land_mode: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftPrRequest {
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// Hint only — broker generates the real head branch.
    #[serde(default)]
    pub head_branch: Option<String>,
    /// Hint only — base comes from the signed permit.
    #[serde(default)]
    pub base_branch: Option<String>,
    pub artifact: ArtifactRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub kind: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftPrResponse {
    pub pr_url: String,
    pub draft: bool,
    pub head_branch: String,
    pub artifact_hash: String,
}

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("permit_denied: {0}")]
    PermitDenied(String),
    #[error("allow_once_required")]
    AllowOnceRequired,
    #[error("allow_once_denied")]
    AllowOnceDenied,
    #[error("artifact_mismatch")]
    ArtifactMismatch,
    #[error("expired")]
    Expired,
    #[error("github_unconfigured: {0}")]
    GithubUnconfigured(String),
    #[error("base_moved: permit base_sha no longer matches remote/local base")]
    BaseMoved,
    #[error("broker: {0}")]
    Msg(String),
}

#[derive(Debug, Clone)]
pub struct BrokerOptions {
    pub session: BrokerSession,
    /// Presented bearer (without "Bearer " prefix).
    pub run_token: String,
    /// When set, auto-approve only if it equals the current artifact hash (tests).
    pub allow_once_override: Option<String>,
    /// Skip `gh` and return a fake URL (unit tests / dry-run).
    pub dry_run: bool,
    /// Interactive allow-once via stdin (false in HTTP unless tty).
    pub interactive: bool,
}

/// Compute sha256 of the review diff file (artifact bind).
pub fn artifact_hash_of_diff(diff_path: &Path) -> Result<String, BrokerError> {
    let bytes = fs::read(diff_path).map_err(|e| BrokerError::Msg(e.to_string()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(format!("sha256:{:x}", h.finalize()))
}

/// Anti-fatigue allow-once summary for the operator.
pub fn allow_once_summary(diff_path: &Path, artifact_hash: &str, head: &str, base: &str) -> String {
    let diff = fs::read_to_string(diff_path).unwrap_or_default();
    let mut files = 0usize;
    let mut added = 0i64;
    let mut removed = 0i64;
    let mut exec_hits: Vec<&str> = Vec::new();
    for line in diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") {
            if line.starts_with("+++ ") && !line.contains("/dev/null") {
                files += 1;
                let path = line.trim_start_matches("+++ b/").trim_start_matches("+++ ");
                if is_execution_bearing(path) {
                    exec_hits.push(path);
                }
            }
        } else if let Some(rest) = line.strip_prefix('+') {
            if !rest.starts_with("++") {
                added += 1;
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if !rest.starts_with("--") {
                removed += 1;
            }
        }
    }
    let short = artifact_hash.get(7..15).unwrap_or(artifact_hash);
    let mut s = format!(
        "Allow-once draft PR?\n  files≈{files}  +{added}/-{removed}  artifact={short}…\n  {head} → {base} (draft only — no merge)\n"
    );
    if !exec_hits.is_empty() {
        s.push_str("  ⚠ touches execution-bearing paths:\n");
        for p in exec_hits.iter().take(8) {
            s.push_str(&format!("    - {p}\n"));
        }
    }
    s.push_str("Confirm draft PR? [y/N] ");
    s
}

fn is_execution_bearing(path: &str) -> bool {
    let p = path.trim();
    p.contains(".github/workflows/")
        || p.contains(".git/hooks/")
        || p.ends_with("package.json")
        || p.ends_with("Makefile")
        || p.ends_with(".envrc")
        || p.contains(".vscode/tasks.json")
}

/// Materialize staged tree into a clean host-owned repo (never agent `.git`).
pub fn materialize_clean_host_repo(
    staged: &Path,
    work_root: &Path,
    base_sha: &str,
    live_repo: &Path,
) -> Result<PathBuf, BrokerError> {
    fs::create_dir_all(work_root).map_err(|e| BrokerError::Msg(e.to_string()))?;
    let dest = work_root.join(format!("land-{}", short_rand()));
    fs::create_dir_all(&dest).map_err(|e| BrokerError::Msg(e.to_string()))?;

    // Prefer cloning from live repo at base_sha into clean dir, then overlay staged tree.
    let status = Command::new("git")
        .args([
            "clone",
            "--no-local",
            "--no-hardlinks",
            live_repo.to_str().unwrap_or("."),
            dest.to_str().unwrap_or("."),
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .map_err(|e| BrokerError::Msg(e.to_string()))?;
    if !status.success() {
        // Fallback: git init + archive from live at base_sha
        let _ = fs::remove_dir_all(&dest);
        fs::create_dir_all(&dest).map_err(|e| BrokerError::Msg(e.to_string()))?;
        git_ok(live_repo, &["init"], &dest)?;
        // copy objects via archive
        let archive = Command::new("git")
            .args(["-C", live_repo.to_str().unwrap_or("."), "archive", base_sha])
            .output()
            .map_err(|e| BrokerError::Msg(e.to_string()))?;
        if !archive.status.success() {
            return Err(BrokerError::Msg("git archive base_sha failed".into()));
        }
        let mut tar = Command::new("tar")
            .args(["-x", "-C", dest.to_str().unwrap_or(".")])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| BrokerError::Msg(e.to_string()))?;
        {
            use std::io::Write as _;
            tar.stdin
                .as_mut()
                .unwrap()
                .write_all(&archive.stdout)
                .map_err(|e| BrokerError::Msg(e.to_string()))?;
        }
        let _ = tar.wait();
        git_ok(&dest, &["add", "-A"], &dest)?;
        git_ok(
            &dest,
            &["-c", "user.email=kotro@local", "-c", "user.name=kotro", "commit", "-m", "base"],
            &dest,
        )?;
    } else {
        // checkout base_sha in clean clone
        let co = Command::new("git")
            .args(["-C", dest.to_str().unwrap_or("."), "checkout", "--force", base_sha])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .map_err(|e| BrokerError::Msg(e.to_string()))?;
        if !co.success() {
            return Err(BrokerError::BaseMoved);
        }
    }

    // Disable hooks in clean repo.
    let hooks = dest.join(".git/hooks");
    if hooks.exists() {
        let _ = fs::remove_dir_all(&hooks);
        let _ = fs::create_dir_all(&hooks);
    }

    // Overlay staged tree (no .git from staging — staging has none).
    copy_tree(staged, &dest)?;
    if dest.join(".git").join("config").exists() {
        // Ensure we did not copy agent .git (staging shouldn't have it).
    }
    Ok(dest)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), BrokerError> {
    for entry in walkdir_simple(src)? {
        let rel = entry.strip_prefix(src).map_err(|e| BrokerError::Msg(e.to_string()))?;
        if rel.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let target = dst.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| BrokerError::Msg(e.to_string()))?;
        } else if entry.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| BrokerError::Msg(e.to_string()))?;
            }
            fs::copy(&entry, &target).map_err(|e| BrokerError::Msg(e.to_string()))?;
        }
    }
    Ok(())
}

fn walkdir_simple(root: &Path) -> Result<Vec<PathBuf>, BrokerError> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BrokerError> {
        for ent in fs::read_dir(dir).map_err(|e| BrokerError::Msg(e.to_string()))? {
            let ent = ent.map_err(|e| BrokerError::Msg(e.to_string()))?;
            let p = ent.path();
            out.push(p.clone());
            if p.is_dir() {
                rec(&p, out)?;
            }
        }
        Ok(())
    }
    rec(root, &mut out)?;
    Ok(out)
}

/// Full thin-broker draft-PR flow.
pub fn handle_draft_pr(opts: &BrokerOptions, req: &DraftPrRequest) -> Result<DraftPrResponse, BrokerError> {
    let session = &opts.session;

    // L2: land mode
    if session.land_mode != "draft_pr" {
        return Err(BrokerError::PermitDenied(
            "land.mode is not draft_pr".into(),
        ));
    }

    // Load envelope for revalidation of land + time
    let raw = fs::read(&session.permit_path).map_err(|e| BrokerError::Msg(e.to_string()))?;
    let envelope: TaskEnvelope =
        parse_envelope_bytes(&raw).map_err(|e| BrokerError::Msg(e.to_string()))?;
    match envelope.land.as_ref().map(|l| l.mode) {
        Some(LandMode::DraftPr) => {}
        _ => return Err(BrokerError::PermitDenied("envelope land.mode".into())),
    }
    // Expiry
    let now = crate::flight_recorder::now_rfc3339();
    kotro_types::check_envelope_time_window(
        &envelope.issued_at,
        &envelope.not_before,
        &envelope.expires_at,
        &now,
    )
    .map_err(|_| BrokerError::Expired)?;

    // L1: run token
    let ok = verify_run_token(
        &session.ledger_dir,
        &session.permit_digest,
        &session.run_id,
        &opts.run_token,
    )
    .map_err(|_| BrokerError::Unauthorized)?;
    if !ok {
        return Err(BrokerError::Unauthorized);
    }

    // L4: artifact hash bind
    let expected = artifact_hash_of_diff(&session.review_diff)?;
    if req.artifact.hash != expected {
        return Err(BrokerError::ArtifactMismatch);
    }

    // Credentials scope (host mediator) — require github draft_pr if listed; if empty, allow with warning path
    let has_github_cred = envelope
        .capabilities
        .credentials
        .iter()
        .any(|c| c.id == "github" && c.scopes.iter().any(|s| s == "draft_pr"));
    let merge_scope = envelope
        .capabilities
        .credentials
        .iter()
        .any(|c| c.scopes.iter().any(|s| s == "merge"));
    if merge_scope {
        return Err(BrokerError::PermitDenied("merge scope forbidden in alpha".into()));
    }
    if !envelope.capabilities.credentials.is_empty() && !has_github_cred {
        return Err(BrokerError::PermitDenied(
            "credentials do not allow github draft_pr".into(),
        ));
    }

    let head = format!("kotro/{}", sanitize_branch(&session.run_id));

    // L3: allow-once anti-fatigue
    let summary = allow_once_summary(
        &session.review_diff,
        &expected,
        &head,
        &session.base_ref,
    );
    let approved = if let Some(h) = &opts.allow_once_override {
        h == &expected
    } else if opts.interactive {
        eprint!("{summary}");
        let _ = io::stdout().flush();
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| BrokerError::Msg(e.to_string()))?;
        matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    } else {
        return Err(BrokerError::AllowOnceRequired);
    };
    if !approved {
        return Err(BrokerError::AllowOnceDenied);
    }

    // Revalidate immediately before side effect
    kotro_types::check_envelope_time_window(
        &envelope.issued_at,
        &envelope.not_before,
        &envelope.expires_at,
        &crate::flight_recorder::now_rfc3339(),
    )
    .map_err(|_| BrokerError::Expired)?;
    let ok2 = verify_run_token(
        &session.ledger_dir,
        &session.permit_digest,
        &session.run_id,
        &opts.run_token,
    )
    .map_err(|_| BrokerError::Unauthorized)?;
    if !ok2 {
        return Err(BrokerError::Unauthorized);
    }
    let expected2 = artifact_hash_of_diff(&session.review_diff)?;
    if expected2 != expected {
        return Err(BrokerError::ArtifactMismatch);
    }

    if opts.dry_run {
        return Ok(DraftPrResponse {
            pr_url: format!("https://example.invalid/{}/pull/dry-run", session.repository_identity),
            draft: true,
            head_branch: head,
            artifact_hash: expected,
        });
    }

    // Verify base_sha still matches live repo
    let live_base = git_stdout(
        &session.live_repo,
        &["rev-parse", &session.base_ref],
    )?;
    if live_base != session.base_sha && !session.base_sha.is_empty() {
        // Allow if base_ref resolves to same as recorded when user uses full sha as ref
        let live_sha = git_stdout(&session.live_repo, &["rev-parse", &session.base_sha])
            .unwrap_or_default();
        if live_sha != session.base_sha {
            return Err(BrokerError::BaseMoved);
        }
    }

    let land_root = session
        .ledger_dir
        .join("land-work");
    let clean = materialize_clean_host_repo(
        &session.staged_dir,
        &land_root,
        &session.base_sha,
        &session.live_repo,
    )?;

    // Commit overlay on clean repo
    git_ok(&clean, &["add", "-A"], &clean)?;
    let _ = Command::new("git")
        .args([
            "-C",
            clean.to_str().unwrap_or("."),
            "-c",
            "user.email=kotro@local",
            "-c",
            "user.name=Kotro Broker",
            "commit",
            "-m",
            &req.title,
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status();

    git_ok(&clean, &["checkout", "-B", &head], &clean)?;

    // Push + draft PR via gh (host credentials)
    if std::env::var("GITHUB_TOKEN").is_err() && std::env::var("GH_TOKEN").is_err() {
        return Err(BrokerError::GithubUnconfigured(
            "set GITHUB_TOKEN on the host (never in the agent)".into(),
        ));
    }

    let push = Command::new("git")
        .args([
            "-C",
            clean.to_str().unwrap_or("."),
            "push",
            "-u",
            "origin",
            &head,
            "--force-with-lease",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .map_err(|e| BrokerError::Msg(e.to_string()))?;
    if !push.status.success() {
        return Err(BrokerError::Msg(format!(
            "git push failed: {}",
            String::from_utf8_lossy(&push.stderr)
        )));
    }

    let pr = Command::new("gh")
        .args([
            "pr",
            "create",
            "--draft",
            "--title",
            &req.title,
            "--body",
            &format!(
                "{}\n\n---\nKotro Permit draft PR (artifact {}).\nDraft ≠ no CI — review carefully.\n",
                req.body, expected
            ),
            "--base",
            &session.base_ref,
            "--head",
            &head,
        ])
        .current_dir(&clean)
        .output()
        .map_err(|e| BrokerError::Msg(e.to_string()))?;
    if !pr.status.success() {
        return Err(BrokerError::Msg(format!(
            "gh pr create failed: {}",
            String::from_utf8_lossy(&pr.stderr)
        )));
    }
    let pr_url = String::from_utf8_lossy(&pr.stdout).trim().to_string();

    Ok(DraftPrResponse {
        pr_url,
        draft: true,
        head_branch: head,
        artifact_hash: expected,
    })
}

fn git_ok(repo: &Path, args: &[&str], cwd: &Path) -> Result<(), BrokerError> {
    let mut cmd = Command::new("git");
    if args.first() != Some(&"init") {
        cmd.arg("-C").arg(repo);
    }
    for a in args {
        cmd.arg(a);
    }
    if args.first() == Some(&"init") {
        cmd.current_dir(cwd);
    }
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    let st = cmd.status().map_err(|e| BrokerError::Msg(e.to_string()))?;
    if !st.success() {
        return Err(BrokerError::Msg(format!("git {args:?} failed")));
    }
    Ok(())
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<String, BrokerError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().map_err(|e| BrokerError::Msg(e.to_string()))?;
    if !out.status.success() {
        return Err(BrokerError::Msg(String::from_utf8_lossy(&out.stderr).into()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn sanitize_branch(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(40)
        .collect()
}

fn short_rand() -> String {
    use rand::RngCore;
    let mut b = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut b);
    format!("{:x}{:x}{:x}{:x}", b[0], b[1], b[2], b[3])
}

pub fn write_session(path: &Path, session: &BrokerSession) -> Result<(), BrokerError> {
    let data = serde_json::to_vec_pretty(session).map_err(|e| BrokerError::Msg(e.to_string()))?;
    fs::write(path, data).map_err(|e| BrokerError::Msg(e.to_string()))
}

pub fn load_session(path: &Path) -> Result<BrokerSession, BrokerError> {
    let data = fs::read(path).map_err(|e| BrokerError::Msg(e.to_string()))?;
    serde_json::from_slice(&data).map_err(|e| BrokerError::Msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permit::token::mint_run_token;
    use ed25519_dalek::SigningKey;
    use kotro_types::{
        key_id_for_public_key, public_key_b64, sign_envelope, AgentScope, Budgets, Capabilities,
        Delegation, DelegationSigner, EnvelopePrincipal, EnvelopeSignature, LandAuthority,
        RepositoryAuthority, API_VERSION_V1ALPHA2, KIND,
    };
    use rand::rngs::OsRng;

    fn sha40(c: char) -> String {
        std::iter::repeat(c).take(40).collect()
    }

    #[test]
    fn forged_token_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let _ = mint_run_token(dir.path(), "sha256:p", "run-1").unwrap();
        let session = BrokerSession {
            permit_digest: "sha256:p".into(),
            run_id: "run-1".into(),
            ledger_dir: dir.path().to_path_buf(),
            permit_path: dir.path().join("missing.json"),
            live_repo: dir.path().to_path_buf(),
            staged_dir: dir.path().to_path_buf(),
            review_diff: dir.path().join("x.diff"),
            repository_identity: "github.com/o/r".into(),
            base_ref: "main".into(),
            base_sha: sha40('a'),
            land_mode: "draft_pr".into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
        };
        fs::write(&session.review_diff, b"diff\n").unwrap();
        // Will fail on missing permit before token if we order wrong — write a minimal envelope.
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = TaskEnvelope {
            api_version: API_VERSION_V1ALPHA2.into(),
            kind: KIND.into(),
            task_id: "t".into(),
            audience: "kotro://deployment/acme".into(),
            issuer: "kotro://authority/acme".into(),
            principal: EnvelopePrincipal {
                subject: "u".into(),
                issuer: "https://i".into(),
            },
            agent_scope: AgentScope {
                names: vec!["codex".into()],
                workload_identities: vec![],
            },
            issued_at: "2026-08-01T18:00:00Z".into(),
            not_before: "2026-08-01T18:00:00Z".into(),
            expires_at: "2099-08-01T19:00:00Z".into(),
            nonce: "CCCCCCCCCCCCCCCCCCCCCC".into(),
            depth: 0,
            parent: None,
            repository: Some(RepositoryAuthority {
                identity: "github.com/o/r".into(),
                source_pin: sha40('a'),
                base_ref: "main".into(),
                base_sha: sha40('a'),
            }),
            land: Some(LandAuthority {
                mode: LandMode::DraftPr,
            }),
            capabilities: Capabilities {
                tools: vec![],
                models: vec![],
                destinations: vec![],
                credentials: vec![],
                filesystem: vec![],
                budgets: Some(Budgets {
                    max_tool_calls: 1,
                    max_model_calls: 1,
                    max_input_tokens: 1,
                    max_output_tokens: 1,
                    max_cost_microusd: 1,
                    max_duration_seconds: 60,
                }),
            },
            delegation: Delegation {
                max_depth: 0,
                signers: vec![DelegationSigner {
                    key_id: key_id_for_public_key(sk.verifying_key().as_bytes()),
                    public_key: public_key_b64(&sk.verifying_key()),
                }],
            },
            signature: EnvelopeSignature {
                algorithm: "Ed25519".into(),
                key_id: String::new(),
                value: String::new(),
            },
        };
        sign_envelope(&mut env, &sk).unwrap();
        fs::write(&session.permit_path, serde_json::to_vec(&env).unwrap()).unwrap();
        let hash = artifact_hash_of_diff(&session.review_diff).unwrap();
        let err = handle_draft_pr(
            &BrokerOptions {
                session,
                run_token: "forged".into(),
                allow_once_override: Some(hash.clone()),
                dry_run: true,
                interactive: false,
            },
            &DraftPrRequest {
                title: "t".into(),
                body: String::new(),
                head_branch: None,
                base_branch: None,
                artifact: ArtifactRef {
                    kind: "review_diff".into(),
                    hash,
                },
            },
        )
        .unwrap_err();
        assert!(matches!(err, BrokerError::Unauthorized));
    }

    #[test]
    fn artifact_mismatch_and_happy_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let tok = mint_run_token(dir.path(), "sha256:p2", "run-2").unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = TaskEnvelope {
            api_version: API_VERSION_V1ALPHA2.into(),
            kind: KIND.into(),
            task_id: "t".into(),
            audience: "kotro://deployment/acme".into(),
            issuer: "kotro://authority/acme".into(),
            principal: EnvelopePrincipal {
                subject: "u".into(),
                issuer: "https://i".into(),
            },
            agent_scope: AgentScope {
                names: vec!["codex".into()],
                workload_identities: vec![],
            },
            issued_at: "2026-08-01T18:00:00Z".into(),
            not_before: "2026-08-01T18:00:00Z".into(),
            expires_at: "2099-08-01T19:00:00Z".into(),
            nonce: "DDDDDDDDDDDDDDDDDDDDDD".into(),
            depth: 0,
            parent: None,
            repository: Some(RepositoryAuthority {
                identity: "github.com/o/r".into(),
                source_pin: sha40('b'),
                base_ref: "main".into(),
                base_sha: sha40('b'),
            }),
            land: Some(LandAuthority {
                mode: LandMode::DraftPr,
            }),
            capabilities: Capabilities {
                tools: vec![],
                models: vec![],
                destinations: vec![],
                credentials: vec![],
                filesystem: vec![],
                budgets: Some(Budgets {
                    max_tool_calls: 1,
                    max_model_calls: 1,
                    max_input_tokens: 1,
                    max_output_tokens: 1,
                    max_cost_microusd: 1,
                    max_duration_seconds: 60,
                }),
            },
            delegation: Delegation {
                max_depth: 0,
                signers: vec![DelegationSigner {
                    key_id: key_id_for_public_key(sk.verifying_key().as_bytes()),
                    public_key: public_key_b64(&sk.verifying_key()),
                }],
            },
            signature: EnvelopeSignature {
                algorithm: "Ed25519".into(),
                key_id: String::new(),
                value: String::new(),
            },
        };
        sign_envelope(&mut env, &sk).unwrap();
        let permit_path = dir.path().join("permit.json");
        fs::write(&permit_path, serde_json::to_vec(&env).unwrap()).unwrap();
        let diff = dir.path().join("r.diff");
        fs::write(
            &diff,
            "--- a/app.txt\n+++ b/app.txt\n@@ -1 +1 @@\n-old\n+new\n",
        )
        .unwrap();
        let hash = artifact_hash_of_diff(&diff).unwrap();
        let session = BrokerSession {
            permit_digest: "sha256:p2".into(),
            run_id: "run-2".into(),
            ledger_dir: dir.path().to_path_buf(),
            permit_path,
            live_repo: dir.path().to_path_buf(),
            staged_dir: dir.path().to_path_buf(),
            review_diff: diff,
            repository_identity: "github.com/o/r".into(),
            base_ref: "main".into(),
            base_sha: sha40('b'),
            land_mode: "draft_pr".into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
        };
        let mismatch = handle_draft_pr(
            &BrokerOptions {
                session: session.clone(),
                run_token: tok.token.clone(),
                allow_once_override: Some(hash.clone()),
                dry_run: true,
                interactive: false,
            },
            &DraftPrRequest {
                title: "t".into(),
                body: String::new(),
                head_branch: None,
                base_branch: None,
                artifact: ArtifactRef {
                    kind: "review_diff".into(),
                    hash: "sha256:deadbeef".into(),
                },
            },
        )
        .unwrap_err();
        assert!(matches!(mismatch, BrokerError::ArtifactMismatch));

        let ok = handle_draft_pr(
            &BrokerOptions {
                session,
                run_token: tok.token,
                allow_once_override: Some(hash.clone()),
                dry_run: true,
                interactive: false,
            },
            &DraftPrRequest {
                title: "fix typo".into(),
                body: String::new(),
                head_branch: None,
                base_branch: None,
                artifact: ArtifactRef {
                    kind: "review_diff".into(),
                    hash,
                },
            },
        )
        .unwrap();
        assert!(ok.draft);
        assert!(ok.pr_url.contains("pull"));
    }

    #[test]
    fn allow_once_deny_and_merge_forbidden() {
        let dir = tempfile::tempdir().unwrap();
        let tok = mint_run_token(dir.path(), "sha256:p3", "run-3").unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = TaskEnvelope {
            api_version: API_VERSION_V1ALPHA2.into(),
            kind: KIND.into(),
            task_id: "t".into(),
            audience: "kotro://deployment/acme".into(),
            issuer: "kotro://authority/acme".into(),
            principal: EnvelopePrincipal {
                subject: "u".into(),
                issuer: "https://i".into(),
            },
            agent_scope: AgentScope {
                names: vec!["codex".into()],
                workload_identities: vec![],
            },
            issued_at: "2026-08-01T18:00:00Z".into(),
            not_before: "2026-08-01T18:00:00Z".into(),
            expires_at: "2099-08-01T19:00:00Z".into(),
            nonce: "EEEEEEEEEEEEEEEEEEEEEE".into(),
            depth: 0,
            parent: None,
            repository: Some(RepositoryAuthority {
                identity: "github.com/o/r".into(),
                source_pin: sha40('c'),
                base_ref: "main".into(),
                base_sha: sha40('c'),
            }),
            land: Some(LandAuthority {
                mode: LandMode::DraftPr,
            }),
            capabilities: Capabilities {
                tools: vec![],
                models: vec![],
                destinations: vec![],
                credentials: vec![kotro_types::envelope::CredentialCapability {
                    id: "github".into(),
                    scopes: vec!["draft_pr".into(), "merge".into()],
                }],
                filesystem: vec![],
                budgets: Some(Budgets {
                    max_tool_calls: 1,
                    max_model_calls: 1,
                    max_input_tokens: 1,
                    max_output_tokens: 1,
                    max_cost_microusd: 1,
                    max_duration_seconds: 60,
                }),
            },
            delegation: Delegation {
                max_depth: 0,
                signers: vec![DelegationSigner {
                    key_id: key_id_for_public_key(sk.verifying_key().as_bytes()),
                    public_key: public_key_b64(&sk.verifying_key()),
                }],
            },
            signature: EnvelopeSignature {
                algorithm: "Ed25519".into(),
                key_id: String::new(),
                value: String::new(),
            },
        };
        sign_envelope(&mut env, &sk).unwrap();
        let permit_path = dir.path().join("permit.json");
        fs::write(&permit_path, serde_json::to_vec(&env).unwrap()).unwrap();
        let diff = dir.path().join("r.diff");
        fs::write(&diff, "+++ b/x\n+1\n").unwrap();
        let hash = artifact_hash_of_diff(&diff).unwrap();
        let session = BrokerSession {
            permit_digest: "sha256:p3".into(),
            run_id: "run-3".into(),
            ledger_dir: dir.path().to_path_buf(),
            permit_path,
            live_repo: dir.path().to_path_buf(),
            staged_dir: dir.path().to_path_buf(),
            review_diff: diff,
            repository_identity: "github.com/o/r".into(),
            base_ref: "main".into(),
            base_sha: sha40('c'),
            land_mode: "draft_pr".into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
        };
        let merge_err = handle_draft_pr(
            &BrokerOptions {
                session: session.clone(),
                run_token: tok.token.clone(),
                allow_once_override: Some(hash.clone()),
                dry_run: true,
                interactive: false,
            },
            &DraftPrRequest {
                title: "t".into(),
                body: String::new(),
                head_branch: None,
                base_branch: None,
                artifact: ArtifactRef {
                    kind: "review_diff".into(),
                    hash: hash.clone(),
                },
            },
        )
        .unwrap_err();
        assert!(matches!(merge_err, BrokerError::PermitDenied(_)));

        // Strip merge; wrong allow-once hash → denied
        env.capabilities.credentials[0].scopes = vec!["draft_pr".into()];
        sign_envelope(&mut env, &sk).unwrap();
        fs::write(&session.permit_path, serde_json::to_vec(&env).unwrap()).unwrap();
        let deny = handle_draft_pr(
            &BrokerOptions {
                session,
                run_token: tok.token,
                allow_once_override: Some("sha256:wrong".into()),
                dry_run: true,
                interactive: false,
            },
            &DraftPrRequest {
                title: "t".into(),
                body: String::new(),
                head_branch: None,
                base_branch: None,
                artifact: ArtifactRef {
                    kind: "review_diff".into(),
                    hash,
                },
            },
        )
        .unwrap_err();
        assert!(matches!(deny, BrokerError::AllowOnceDenied));
    }

    #[test]
    fn execution_bearing_highlighted() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.diff");
        fs::write(
            &p,
            "+++ b/.github/workflows/ci.yml\n+run: true\n+++ b/README.md\n+hi\n",
        )
        .unwrap();
        let s = allow_once_summary(&p, "sha256:abcdefghijklmnop", "kotro/x", "main");
        assert!(s.contains("execution-bearing"));
        assert!(s.contains(".github/workflows/ci.yml"));
    }
}
