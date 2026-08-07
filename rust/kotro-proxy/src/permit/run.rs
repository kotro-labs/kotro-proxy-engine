//! Fail-closed `run --permit` lifecycle (R0.4).
//!
//! **Ledger claim policy (R2-A handoff):** `--verify-only` and the current
//! “sandbox launch deferred” path must **not** reserve/consume a one-shot
//! permit. Call [`claim_for_sandbox_launch`] only when sandbox launch is
//! committed; then [`PermitLedger::consume`] when the agent PID exists.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use kotro_types::{
    envelope_digest, parse_envelope_bytes, verify, MemoryParentStore, TaskEnvelope, TrustStore,
    VerificationContext, VerifiedAuthority, API_VERSION_V1ALPHA2,
};
use thiserror::Error;

use crate::flight_recorder::now_rfc3339;
use crate::permit::ledger::{LedgerError, PermitLedger};
use crate::permit::sandbox::{sandbox_backend_available, SandboxStatus};

#[derive(Debug, Clone)]
pub struct RunPermitOptions {
    pub permit_path: PathBuf,
    pub trust_path: PathBuf,
    pub audience: Option<String>,
    pub parent_store_dir: Option<PathBuf>,
    pub ledger_dir: PathBuf,
    pub agent_cmd: Vec<String>,
    /// Verify + sandbox probe only — **does not** claim the one-shot ledger.
    pub verify_only: bool,
    /// Gates only (no stage/launch). Ledger unclaimed. Exit 2 when used from CLI.
    pub prepare_only: bool,
    /// Live repo to stage (Option A). Required for full R2-A launch.
    pub repo: Option<PathBuf>,
    pub staging_root: PathBuf,
    pub image: String,
    pub memory: String,
    pub cpus: String,
    pub pids_limit: String,
    pub keep_staging: bool,
    /// Injected clock for tests (`None` → wall clock).
    pub now_rfc3339: Option<String>,
    /// Override sandbox probe (tests).
    pub sandbox_override: Option<SandboxStatus>,
    /// When true, refuse (tests / CLI set from `KOTRO_PERMIT_ALLOW_HOST_FALLBACK`).
    pub host_fallback_requested: bool,
    /// Skip real docker; used by unit tests.
    pub skip_docker_launch: bool,
}

#[derive(Debug)]
pub struct PreparedRun {
    pub authority: VerifiedAuthority,
    pub permit_digest: String,
    pub run_id: String,
    pub sandbox: SandboxStatus,
    /// True only after [`claim_for_sandbox_launch`].
    pub ledger_claimed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPermitOutcome {
    /// Full R2-A run completed inside sandbox; review artifact ready for `apply`.
    Completed {
        permit_digest: String,
        run_id: String,
        agent_exit_code: i32,
        staged_dir: String,
        review_diff: String,
        pin: String,
    },
    /// Gates passed; `--prepare-only` — **ledger not claimed**.
    /// CLI maps this to exit code **2** = verified but execution unavailable.
    Prepared {
        permit_digest: String,
        run_id: String,
        sandbox_detail: String,
    },
    /// `--verify-only` — **ledger not claimed**.
    VerifyOnly {
        permit_digest: String,
        run_id: String,
    },
}

#[derive(Debug, Error)]
pub enum RunPermitError {
    #[error("run --permit requires --permit <envelope>")]
    MissingPermit,
    #[error("run --permit requires --trust <trust-store.json>")]
    MissingTrust,
    #[error("read permit: {0}")]
    ReadPermit(String),
    #[error("parse permit: {0}")]
    Parse(String),
    #[error("trust store: {0}")]
    Trust(String),
    #[error("verify failed: {0}")]
    Verify(String),
    #[error("run --permit accepts kotro.dev/v1alpha2 only (got {0})")]
    ApiVersion(String),
    #[error("v1alpha2 permit missing signed repository/land authority")]
    MissingPermitAuthority,
    #[error("ledger: {0}")]
    Ledger(String),
    #[error("sandbox unavailable — refusing host execution: {0}")]
    SandboxUnavailable(String),
    #[error("host fallback is forbidden (ignored KOTRO_PERMIT_ALLOW_HOST_FALLBACK)")]
    HostFallbackForbidden,
    #[error("agent command empty; pass `-- <agent…>` or use --verify-only")]
    EmptyAgent,
    #[error("run --permit requires --repo <path> for sandbox launch (or --verify-only / --prepare-only)")]
    MissingRepo,
    #[error("staging failed: {0}")]
    Staging(String),
    #[error("sandbox launch failed: {0}")]
    Launch(String),
    #[error("land.mode is draft_pr — broker land is R2-B; R2-A produces review diff for apply")]
    DraftPrDeferred,
}

impl From<LedgerError> for RunPermitError {
    fn from(e: LedgerError) -> Self {
        RunPermitError::Ledger(e.to_string())
    }
}

/// Verify v1alpha2 and probe sandbox. **Does not** claim the one-shot ledger.
pub fn prepare_run(opts: &RunPermitOptions) -> Result<PreparedRun, RunPermitError> {
    if opts.permit_path.as_os_str().is_empty() {
        return Err(RunPermitError::MissingPermit);
    }
    if opts.trust_path.as_os_str().is_empty() {
        return Err(RunPermitError::MissingTrust);
    }

    if opts.host_fallback_requested {
        return Err(RunPermitError::HostFallbackForbidden);
    }

    let raw = fs::read(&opts.permit_path)
        .map_err(|e| RunPermitError::ReadPermit(format!("{}: {e}", opts.permit_path.display())))?;
    let envelope =
        parse_envelope_bytes(&raw).map_err(|r| RunPermitError::Parse(r.to_string()))?;

    if envelope.api_version != API_VERSION_V1ALPHA2 {
        return Err(RunPermitError::ApiVersion(envelope.api_version.clone()));
    }
    if envelope.repository.is_none() || envelope.land.is_none() {
        return Err(RunPermitError::MissingPermitAuthority);
    }

    let trust =
        TrustStore::load(&opts.trust_path).map_err(|r| RunPermitError::Trust(r.to_string()))?;
    let mut parents = MemoryParentStore::default();
    if let Some(dir) = &opts.parent_store_dir {
        load_parent_dir(dir, &mut parents)?;
    }

    let now = opts.now_rfc3339.clone().unwrap_or_else(now_rfc3339);
    let ctx = VerificationContext {
        trust: &trust,
        parents: &parents,
        now_rfc3339: &now,
        expected_audience: opts.audience.as_deref(),
        kill_engaged: false,
    };
    let authority = verify(&envelope, &ctx).map_err(|r| RunPermitError::Verify(r.to_string()))?;
    let permit_digest = authority.digest.clone();
    let run_id = format!("run-{}", short_id());

    let sandbox = opts
        .sandbox_override
        .clone()
        .unwrap_or_else(sandbox_backend_available);
    if !opts.verify_only {
        if let SandboxStatus::Unavailable { reason } = &sandbox {
            return Err(RunPermitError::SandboxUnavailable(reason.clone()));
        }
    }

    Ok(PreparedRun {
        authority,
        permit_digest,
        run_id,
        sandbox,
        ledger_claimed: false,
    })
}

/// Claim the one-shot ledger **only when sandbox launch is committed** (R2-A).
///
/// Call after [`prepare_run`] succeeds and immediately before starting the
/// container. On Docker/pre-agent failure, call [`PermitLedger::release_pre_agent`].
/// When the agent PID exists, call [`PermitLedger::consume`].
pub fn claim_for_sandbox_launch(
    opts: &RunPermitOptions,
    prepared: &mut PreparedRun,
) -> Result<(), RunPermitError> {
    let now = opts.now_rfc3339.clone().unwrap_or_else(now_rfc3339);
    revalidate_time(&prepared.authority.envelope, &now)?;
    let ledger = PermitLedger::open(&opts.ledger_dir)?;
    ledger.reserve(&prepared.permit_digest, &prepared.run_id, &now)?;
    prepared.ledger_claimed = true;
    Ok(())
}

/// Full CLI entry: verify; claim+launch only when executing R2-A sandbox.
pub fn run_permit(opts: RunPermitOptions) -> Result<RunPermitOutcome, RunPermitError> {
    let mut prepared = prepare_run(&opts)?;
    let now = opts.now_rfc3339.clone().unwrap_or_else(now_rfc3339);
    revalidate_time(&prepared.authority.envelope, &now)?;

    debug_assert!(
        !prepared.ledger_claimed,
        "prepare_run must not claim the ledger"
    );

    if opts.verify_only {
        return Ok(RunPermitOutcome::VerifyOnly {
            permit_digest: prepared.permit_digest,
            run_id: prepared.run_id,
        });
    }

    if opts.prepare_only {
        let detail = match &prepared.sandbox {
            SandboxStatus::Available { detail } => detail.clone(),
            SandboxStatus::Unavailable { reason } => reason.clone(),
        };
        return Ok(RunPermitOutcome::Prepared {
            permit_digest: prepared.permit_digest,
            run_id: prepared.run_id,
            sandbox_detail: detail,
        });
    }

    if opts.agent_cmd.is_empty() {
        return Err(RunPermitError::EmptyAgent);
    }

    let repo = opts.repo.clone().ok_or(RunPermitError::MissingRepo)?;

    match &prepared.sandbox {
        SandboxStatus::Unavailable { reason } => {
            return Err(RunPermitError::SandboxUnavailable(reason.clone()));
        }
        SandboxStatus::Available { .. } => {}
    }

    // Option A stage (before claim — staging failure must not consume permit).
    let pin_rev = prepared
        .authority
        .envelope
        .repository
        .as_ref()
        .map(|r| r.source_pin.as_str())
        .unwrap_or("HEAD");
    let stage = crate::permit::stage::stage_repo(&crate::permit::stage::StageOptions {
        repo: repo.clone(),
        rev: pin_rev.to_string(),
        staging_root: opts.staging_root.clone(),
        keep_baseline: true,
    })
    .map_err(|e| RunPermitError::Staging(e.to_string()))?;

    crate::permit::docker::refuse_dangerous_mount(&stage.staged_dir)
        .map_err(|e| RunPermitError::Launch(e.to_string()))?;

    let pin = stage.pin.clone();
    claim_for_sandbox_launch(&opts, &mut prepared)?;
    let ledger = PermitLedger::open(&opts.ledger_dir)?;

    let mut env = HashMap::new();
    // Do not inject host filesystem paths for envelope/trust into the agent —
    // those files are not mounted. TaskGate-inside-sandbox is a later wire-up.
    env.insert("KOTRO_TASK_REQUIRED".into(), "true".into());
    env.insert("KOTRO_PERMIT_DIGEST".into(), prepared.permit_digest.clone());
    env.insert("KOTRO_RUN_ID".into(), prepared.run_id.clone());
    if let Some(aud) = &opts.audience {
        env.insert("KOTRO_TASK_AUDIENCE".into(), aud.clone());
    }
    env.insert("KOTRO_WORKSPACE".into(), "/workspace".into());
    env.insert("KOTRO_PERMIT_PIN".into(), pin.clone());

    let launch = if opts.skip_docker_launch {
        // Test hook: pretend successful empty run without docker.
        Ok(crate::permit::docker::DockerRunResult {
            exit_code: 0,
            network: "none".into(),
            container_name: "test".into(),
        })
    } else {
        let docker_opts = crate::permit::docker::DockerRunOptions {
            run_id: prepared.run_id.clone(),
            image: opts.image.clone(),
            workspace: stage.staged_dir.clone(),
            workdir: "/workspace".into(),
            agent_cmd: opts.agent_cmd.clone(),
            env,
            memory: opts.memory.clone(),
            cpus: opts.cpus.clone(),
            pids_limit: opts.pids_limit.clone(),
            network_none: false,
        };
        match crate::permit::docker::run_agent_container(&docker_opts) {
            Ok(r) => Ok(r),
            Err(e) => {
                let _ = ledger.release_pre_agent(&prepared.permit_digest, &prepared.run_id);
                Err(RunPermitError::Launch(e.to_string()))
            }
        }
    };

    let docker_result = launch?;

    // Container started (or test skip) → consume one-shot claim.
    let now = opts.now_rfc3339.clone().unwrap_or_else(now_rfc3339);
    if let Err(e) = ledger.consume(&prepared.permit_digest, &prepared.run_id, &now) {
        eprintln!("warning: ledger consume failed: {e}");
    }

    let review_diff = PathBuf::from(format!("{}.review.diff", stage.staged_dir.display()));
    if let Some(baseline) = &stage.baseline_dir {
        crate::permit::stage::write_review_diff(baseline, &stage.staged_dir, &review_diff)
            .map_err(|e| RunPermitError::Staging(e.to_string()))?;
    }

    if !opts.keep_staging {
        // Keep staging + review diff by default for apply; only drop baseline.
        if let Some(b) = &stage.baseline_dir {
            let _ = std::fs::remove_dir_all(b);
        }
    }

    let _ = &prepared.authority; // land.mode draft_pr still gets apply artifact in R2-A
    let _ = RunPermitError::DraftPrDeferred;

    Ok(RunPermitOutcome::Completed {
        permit_digest: prepared.permit_digest,
        run_id: prepared.run_id,
        agent_exit_code: docker_result.exit_code,
        staged_dir: stage.staged_dir.display().to_string(),
        review_diff: review_diff.display().to_string(),
        pin,
    })
}

fn revalidate_time(envelope: &TaskEnvelope, now: &str) -> Result<(), RunPermitError> {
    kotro_types::check_envelope_time_window(
        &envelope.issued_at,
        &envelope.not_before,
        &envelope.expires_at,
        now,
    )
    .map_err(|r| RunPermitError::Verify(r.to_string()))
}

/// Env vars for MCP TaskGate inside a future sandbox (enforce, not fail-open).
pub fn task_gate_env(opts: &RunPermitOptions, prepared: &PreparedRun) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("KOTRO_TASK_REQUIRED".into(), "true".into());
    env.insert(
        "KOTRO_TASK_ENVELOPE".into(),
        opts.permit_path.display().to_string(),
    );
    env.insert(
        "KOTRO_TRUST_STORE".into(),
        opts.trust_path.display().to_string(),
    );
    if let Some(aud) = &opts.audience {
        env.insert("KOTRO_TASK_AUDIENCE".into(), aud.clone());
    }
    env.insert("KOTRO_PERMIT_DIGEST".into(), prepared.permit_digest.clone());
    env.insert("KOTRO_RUN_ID".into(), prepared.run_id.clone());
    env
}

fn short_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

fn load_parent_dir(
    dir: &Path,
    parents: &mut MemoryParentStore,
) -> Result<(), RunPermitError> {
    let entries = fs::read_dir(dir).map_err(|e| RunPermitError::Trust(e.to_string()))?;
    for ent in entries.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|e| RunPermitError::Trust(e.to_string()))?;
        let env = parse_envelope_bytes(&raw).map_err(|r| RunPermitError::Parse(r.to_string()))?;
        let digest = envelope_digest(&env).map_err(|e| RunPermitError::Parse(e))?;
        parents.entries.insert(digest, env);
    }
    Ok(())
}

// tiny hex without extra crate
mod hex {
    pub fn encode(bytes: [u8; 8]) -> String {
        const HEX: &[u8] = b"0123456789abcdef";
        let mut s = String::with_capacity(16);
        for b in bytes {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0xf) as usize] as char);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permit::ledger::{PermitLedger, PermitLedgerState};
    use ed25519_dalek::SigningKey;
    use kotro_types::{
        check_non_expansion, key_id_for_public_key, public_key_b64, sign_envelope, signing_input,
        unsigned_value, AgentScope, Budgets, Capabilities, Delegation, DelegationSigner,
        DestinationCapability, EnvelopePrincipal, EnvelopeSignature, LandAuthority, LandMode,
        RepositoryAuthority, ToolCapability, TrustKey, API_VERSION, API_VERSION_V1ALPHA1, KIND,
        SIGNING_DOMAIN_V1ALPHA1, SIGNING_DOMAIN_V1ALPHA2,
    };
    use rand::rngs::OsRng;

    fn budgets() -> Budgets {
        Budgets {
            max_tool_calls: 10,
            max_model_calls: 10,
            max_input_tokens: 1000,
            max_output_tokens: 1000,
            max_cost_microusd: 1000,
            max_duration_seconds: 600,
        }
    }

    fn sha40(c: char) -> String {
        std::iter::repeat(c).take(40).collect()
    }

    fn sample_v1alpha2(sk: &SigningKey) -> TaskEnvelope {
        let vk = sk.verifying_key();
        let mut env = TaskEnvelope {
            api_version: API_VERSION_V1ALPHA2.into(),
            kind: KIND.into(),
            task_id: "task-permit".into(),
            audience: "kotro://deployment/acme".into(),
            issuer: "kotro://authority/acme".into(),
            principal: EnvelopePrincipal {
                subject: "user@example.com".into(),
                issuer: "https://identity.example.com".into(),
            },
            agent_scope: AgentScope {
                names: vec!["codex".into()],
                workload_identities: vec![],
            },
            issued_at: "2026-08-01T18:00:00Z".into(),
            not_before: "2026-08-01T18:00:00Z".into(),
            expires_at: "2026-08-01T19:00:00Z".into(),
            nonce: "BBBBBBBBBBBBBBBBBBBBBB".into(),
            depth: 0,
            parent: None,
            repository: Some(RepositoryAuthority {
                identity: "github.com/kotro-labs/kotro-proxy-engine".into(),
                source_pin: sha40('a'),
                base_ref: "refs/heads/main".into(),
                base_sha: sha40('b'),
            }),
            land: Some(LandAuthority {
                mode: LandMode::DraftPr,
            }),
            capabilities: Capabilities {
                tools: vec![ToolCapability {
                    server: "github".into(),
                    name: "noop".into(),
                    tool_schema_sha256: None,
                    arguments: None,
                }],
                models: vec![],
                destinations: vec![DestinationCapability {
                    scheme: "https".into(),
                    host: "broker.kotro.local".into(),
                    port: 443,
                    path_prefix: Some("/".into()),
                }],
                credentials: vec![],
                filesystem: vec![],
                budgets: Some(budgets()),
            },
            delegation: Delegation {
                max_depth: 2,
                signers: vec![DelegationSigner {
                    key_id: key_id_for_public_key(vk.as_bytes()),
                    public_key: public_key_b64(&vk),
                }],
            },
            signature: EnvelopeSignature {
                algorithm: "Ed25519".into(),
                key_id: String::new(),
                value: String::new(),
            },
        };
        sign_envelope(&mut env, sk).unwrap();
        env
    }

    fn write_fixture(dir: &Path, sk: &SigningKey, env: &TaskEnvelope) -> (PathBuf, PathBuf) {
        let permit = dir.join("permit.json");
        let trust = dir.join("trust.json");
        fs::write(&permit, serde_json::to_vec_pretty(env).unwrap()).unwrap();
        let vk = sk.verifying_key();
        let store = TrustStore {
            keys: vec![TrustKey {
                key_id: key_id_for_public_key(vk.as_bytes()),
                algorithm: "Ed25519".into(),
                public_key: public_key_b64(&vk),
                issuers: vec!["kotro://authority/acme".into()],
                status: "active".into(),
                not_before: "2026-01-01T00:00:00Z".into(),
                not_after: "2027-01-01T00:00:00Z".into(),
            }],
            revoked_key_ids: vec![],
            revoked_task_ids: vec![],
            revoked_envelope_digests: vec![],
        };
        let bytes = serde_json::to_vec_pretty(&store).unwrap();
        fs::write(&trust, &bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&trust).unwrap().permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&trust, perms).unwrap();
        }
        (permit, trust)
    }

    fn opts(permit: PathBuf, trust: PathBuf, ledger: PathBuf) -> RunPermitOptions {
        RunPermitOptions {
            permit_path: permit,
            trust_path: trust,
            audience: Some("kotro://deployment/acme".into()),
            parent_store_dir: None,
            ledger_dir: ledger,
            agent_cmd: vec!["claude".into()],
            verify_only: true,
            prepare_only: false,
            repo: None,
            staging_root: PathBuf::from("/tmp/kotro-staging-test"),
            image: "alpine:3.20".into(),
            memory: "512m".into(),
            cpus: "1".into(),
            pids_limit: "256".into(),
            keep_staging: true,
            now_rfc3339: Some("2026-08-01T18:30:00Z".into()),
            sandbox_override: Some(SandboxStatus::Available {
                detail: "test".into(),
            }),
            host_fallback_requested: false,
            skip_docker_launch: true,
        }
    }

    #[test]
    fn accepts_v1alpha2_only() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let digest = envelope_digest(&env).unwrap();
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let ledger_dir = dir.path().join("ledger");
        let out = run_permit(opts(permit, trust, ledger_dir.clone())).unwrap();
        assert!(matches!(out, RunPermitOutcome::VerifyOnly { .. }));
        let ledger = PermitLedger::open(&ledger_dir).unwrap();
        assert!(
            !ledger.is_claimed(&digest),
            "verify-only must not claim one-shot permit"
        );
    }

    #[test]
    fn prepared_deferred_does_not_claim_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let digest = envelope_digest(&env).unwrap();
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let ledger_dir = dir.path().join("ledger");
        let mut o = opts(permit, trust, ledger_dir.clone());
        o.verify_only = false;
        o.prepare_only = true;
        let out = run_permit(o).unwrap();
        assert!(matches!(out, RunPermitOutcome::Prepared { .. }));
        let ledger = PermitLedger::open(&ledger_dir).unwrap();
        assert!(
            !ledger.is_claimed(&digest),
            "prepare-only must not claim one-shot permit"
        );
    }

    #[test]
    fn r2a_completed_claims_and_writes_review_diff() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(&repo)
            .status()
            .unwrap();
        fs::write(repo.join("README.md"), "ok\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&repo)
            .status()
            .unwrap();
        let pin = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_v1alpha2(&sk);
        env.repository.as_mut().unwrap().source_pin = pin.clone();
        env.repository.as_mut().unwrap().base_sha = pin.clone();
        env.land.as_mut().unwrap().mode = LandMode::ApplyOnly;
        sign_envelope(&mut env, &sk).unwrap();
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let ledger_dir = dir.path().join("ledger");
        let mut o = opts(permit, trust, ledger_dir.clone());
        o.verify_only = false;
        o.prepare_only = false;
        o.repo = Some(repo);
        o.staging_root = dir.path().join("staging");
        o.agent_cmd = vec!["true".into()];
        o.skip_docker_launch = true;
        let out = run_permit(o).unwrap();
        match out {
            RunPermitOutcome::Completed {
                review_diff,
                agent_exit_code,
                ..
            } => {
                assert_eq!(agent_exit_code, 0);
                assert!(PathBuf::from(&review_diff).is_file());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        let digest = envelope_digest(&env).unwrap();
        let ledger = PermitLedger::open(&ledger_dir).unwrap();
        assert_eq!(
            ledger.load(&digest).unwrap().state,
            PermitLedgerState::Consumed
        );
    }

    #[test]
    fn claim_only_when_sandbox_launch_committed() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let ledger_dir = dir.path().join("ledger");
        let mut o = opts(permit, trust, ledger_dir.clone());
        o.verify_only = false;
        let mut prepared = prepare_run(&o).unwrap();
        assert!(!prepared.ledger_claimed);
        claim_for_sandbox_launch(&o, &mut prepared).unwrap();
        assert!(prepared.ledger_claimed);
        let ledger = PermitLedger::open(&ledger_dir).unwrap();
        assert!(ledger.is_claimed(&prepared.permit_digest));
        assert_eq!(
            ledger.load(&prepared.permit_digest).unwrap().state,
            PermitLedgerState::Reserved
        );
        // Replay claim by another run fails.
        let mut other = prepare_run(&o).unwrap();
        other.run_id = "run-other".into();
        assert!(claim_for_sandbox_launch(&o, &mut other).is_err());
    }

    #[test]
    fn rejects_v1alpha1() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_v1alpha2(&sk);
        env.api_version = API_VERSION_V1ALPHA1.into();
        env.repository = None;
        env.land = None;
        sign_envelope(&mut env, &sk).unwrap();
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let err = run_permit(opts(permit, trust, dir.path().join("ledger"))).unwrap_err();
        assert!(matches!(err, RunPermitError::ApiVersion(_)));
    }

    #[test]
    fn rejects_v1alpha1_with_permit_fields() {
        // Shape-level: v1alpha1 + repository must fail verify / parse path.
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_v1alpha2(&sk);
        env.api_version = API_VERSION.into(); // v1alpha1
        sign_envelope(&mut env, &sk).unwrap();
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let err = run_permit(opts(permit, trust, dir.path().join("ledger"))).unwrap_err();
        assert!(matches!(err, RunPermitError::ApiVersion(_)));
    }

    #[test]
    fn sandbox_absent_never_runs_host_agent() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let marker = dir.path().join("HOST_EXECUTED");
        let mut o = opts(permit, trust, dir.path().join("ledger"));
        o.verify_only = false;
        o.sandbox_override = Some(SandboxStatus::Unavailable {
            reason: "forced".into(),
        });
        o.agent_cmd = vec![
            "bash".into(),
            "-c".into(),
            format!("touch {}", marker.display()),
        ];
        let err = run_permit(o).unwrap_err();
        assert!(matches!(err, RunPermitError::SandboxUnavailable(_)));
        assert!(!marker.exists(), "host agent must not execute");
    }

    #[test]
    fn host_fallback_env_forbidden() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let mut o = opts(permit, trust, dir.path().join("ledger"));
        o.host_fallback_requested = true;
        let err = run_permit(o).unwrap_err();
        assert!(matches!(err, RunPermitError::HostFallbackForbidden));
    }

    #[test]
    fn replay_after_consume_fails() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let ledger_dir = dir.path().join("ledger");
        let mut o = opts(permit, trust, ledger_dir.clone());
        o.verify_only = false;
        let mut prepared = prepare_run(&o).unwrap();
        claim_for_sandbox_launch(&o, &mut prepared).unwrap();
        let ledger = PermitLedger::open(&ledger_dir).unwrap();
        ledger
            .consume(
                &prepared.permit_digest,
                &prepared.run_id,
                "2026-08-01T18:01:00Z",
            )
            .unwrap();
        let mut again = prepare_run(&o).unwrap();
        assert!(matches!(
            claim_for_sandbox_launch(&o, &mut again).unwrap_err(),
            RunPermitError::Ledger(_)
        ));
    }

    #[test]
    fn active_run_expiry_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let (permit, trust) = write_fixture(dir.path(), &sk, &env);
        let mut o = opts(permit, trust, dir.path().join("ledger"));
        o.now_rfc3339 = Some("2026-08-01T20:00:00Z".into());
        let err = run_permit(o).unwrap_err();
        assert!(matches!(err, RunPermitError::Verify(_)));
    }

    #[test]
    fn repo_base_mutation_breaks_verify() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_v1alpha2(&sk);
        env.repository.as_mut().unwrap().base_sha = sha40('z');
        // signature still old → verify fails
        let trust = TrustStore {
            keys: vec![TrustKey {
                key_id: key_id_for_public_key(sk.verifying_key().as_bytes()),
                algorithm: "Ed25519".into(),
                public_key: public_key_b64(&sk.verifying_key()),
                issuers: vec!["kotro://authority/acme".into()],
                status: "active".into(),
                not_before: "2026-01-01T00:00:00Z".into(),
                not_after: "2027-01-01T00:00:00Z".into(),
            }],
            revoked_key_ids: vec![],
            revoked_task_ids: vec![],
            revoked_envelope_digests: vec![],
        };
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert!(verify(&env, &ctx).is_err());
    }

    #[test]
    fn land_narrow_ok_widen_fails() {
        let sk = SigningKey::generate(&mut OsRng);
        let parent = sample_v1alpha2(&sk);
        let mut child = parent.clone();
        child.depth = 1;
        child.land.as_mut().unwrap().mode = LandMode::ApplyOnly;
        assert!(check_non_expansion(&parent, &child).is_ok());
        child.land.as_mut().unwrap().mode = LandMode::DraftPr;
        let mut apply_parent = parent.clone();
        apply_parent.land.as_mut().unwrap().mode = LandMode::ApplyOnly;
        child.land.as_mut().unwrap().mode = LandMode::DraftPr;
        assert!(check_non_expansion(&apply_parent, &child).is_err());
    }

    #[test]
    fn cross_version_signing_domain_rejects() {
        use base64::Engine;
        use ed25519_dalek::Signer;
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_v1alpha2(&sk);
        // Build signing input with the *wrong* (v1alpha1) domain while claiming v1alpha2.
        let unsigned = unsigned_value(&env).unwrap();
        let jcs = serde_json_canonicalizer::to_vec(&unsigned).unwrap();
        let mut msg = Vec::new();
        msg.extend_from_slice(SIGNING_DOMAIN_V1ALPHA1);
        msg.extend_from_slice(&jcs);
        let sig = sk.sign(&msg);
        env.signature.key_id = key_id_for_public_key(sk.verifying_key().as_bytes());
        env.signature.value =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes());
        assert_ne!(SIGNING_DOMAIN_V1ALPHA1, SIGNING_DOMAIN_V1ALPHA2);
        let trust = TrustStore {
            keys: vec![TrustKey {
                key_id: env.signature.key_id.clone(),
                algorithm: "Ed25519".into(),
                public_key: public_key_b64(&sk.verifying_key()),
                issuers: vec!["kotro://authority/acme".into()],
                status: "active".into(),
                not_before: "2026-01-01T00:00:00Z".into(),
                not_after: "2027-01-01T00:00:00Z".into(),
            }],
            revoked_key_ids: vec![],
            revoked_task_ids: vec![],
            revoked_envelope_digests: vec![],
        };
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert!(verify(&env, &ctx).is_err());
        let good = signing_input(&sample_v1alpha2(&sk)).unwrap();
        assert!(good.starts_with(SIGNING_DOMAIN_V1ALPHA2));
    }
}
