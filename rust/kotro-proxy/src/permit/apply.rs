//! Apply a reviewed unified diff to a host repo (R2-A land path).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("apply: {0}")]
    Msg(String),
    #[error("diff file missing: {0}")]
    MissingDiff(String),
    #[error("repo missing: {0}")]
    MissingRepo(String),
}

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub repo: PathBuf,
    pub diff_path: PathBuf,
    /// If true, only `git apply --check` (or patch --dry-run).
    pub check_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    pub applied: bool,
    pub check_only: bool,
}

/// Apply a reviewed artifact to the **live** host repo after human review.
/// Does not run inside the agent sandbox; host-side only.
pub fn apply_review_diff(opts: &ApplyOptions) -> Result<ApplyResult, ApplyError> {
    if !opts.diff_path.is_file() {
        return Err(ApplyError::MissingDiff(opts.diff_path.display().to_string()));
    }
    if !opts.repo.is_dir() {
        return Err(ApplyError::MissingRepo(opts.repo.display().to_string()));
    }
    let diff_bytes = fs::read(&opts.diff_path).map_err(|e| ApplyError::Msg(e.to_string()))?;
    if diff_bytes.is_empty() {
        // Empty diff = nothing to apply (agent made no changes).
        return Ok(ApplyResult {
            applied: false,
            check_only: opts.check_only,
        });
    }

    // Prefer git apply when repo is a work tree.
    let is_git = Command::new("git")
        .args([
            "-C",
            opts.repo.to_str().unwrap_or("."),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if is_git {
        let mut args = vec![
            "-C".to_string(),
            opts.repo.display().to_string(),
            "apply".into(),
        ];
        if opts.check_only {
            args.push("--check".into());
        }
        args.push(opts.diff_path.display().to_string());
        let out = Command::new("git")
            .args(&args)
            .output()
            .map_err(|e| ApplyError::Msg(e.to_string()))?;
        if !out.status.success() {
            return Err(ApplyError::Msg(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        return Ok(ApplyResult {
            applied: !opts.check_only,
            check_only: opts.check_only,
        });
    }

    // Fallback: patch -p1
    let mut args = vec!["-p1".to_string(), "-d".to_string(), opts.repo.display().to_string()];
    if opts.check_only {
        args.push("--dry-run".into());
    }
    args.push("-i".into());
    args.push(opts.diff_path.display().to_string());
    let out = Command::new("patch")
        .args(&args)
        .output()
        .map_err(|e| ApplyError::Msg(e.to_string()))?;
    if !out.status.success() {
        return Err(ApplyError::Msg(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(ApplyResult {
        applied: !opts.check_only,
        check_only: opts.check_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let diff = tmp.path().join("empty.diff");
        fs::write(&diff, b"").unwrap();
        let r = apply_review_diff(&ApplyOptions {
            repo,
            diff_path: diff,
            check_only: false,
        })
        .unwrap();
        assert!(!r.applied);
    }
}
