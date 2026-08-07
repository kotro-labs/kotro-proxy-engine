//! Option A staging — tracked tree at a pin into a Kotro-owned staging root.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StageError {
    #[error("staging I/O: {0}")]
    Io(String),
    #[error("git: {0}")]
    Git(String),
    #[error("not a git work tree: {0}")]
    NotGit(String),
    #[error("staging root refused: {0}")]
    BadRoot(String),
}

#[derive(Debug, Clone)]
pub struct StageOptions {
    pub repo: PathBuf,
    pub rev: String,
    pub staging_root: PathBuf,
    /// Also materialize a pristine baseline sibling for post-run review diffs.
    pub keep_baseline: bool,
}

#[derive(Debug, Clone)]
pub struct StageResult {
    pub staged_dir: PathBuf,
    pub baseline_dir: Option<PathBuf>,
    pub manifest_path: PathBuf,
    pub pin: String,
    pub tracked_count: usize,
}

const DENY_NAMES: &[&str] = &[
    ".env",
    ".git",
    "id_rsa",
    "KOTRO_STAGING_MANIFEST.txt",
];

fn is_denied_name(name: &str) -> bool {
    if DENY_NAMES.iter().any(|d| *d == name) {
        return true;
    }
    name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa.")
}

/// Stage tracked files at `rev` under `$KOTRO_STAGING_ROOT` via `git archive`.
/// Never deletes caller paths; allocates a new `stage.XXXX` directory only.
pub fn stage_repo(opts: &StageOptions) -> Result<StageResult, StageError> {
    let repo = opts
        .repo
        .canonicalize()
        .map_err(|e| StageError::Io(format!("{}: {e}", opts.repo.display())))?;

    let git_ok = Command::new("git")
        .args(["-C", repo.to_str().unwrap_or("."), "rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|e| StageError::Git(e.to_string()))?;
    if !git_ok.status.success() {
        return Err(StageError::NotGit(repo.display().to_string()));
    }

    let pin = git_stdout(&repo, &["rev-parse", &opts.rev])?;
    let tracked = git_stdout(&repo, &["ls-tree", "-r", "--name-only", &pin])?;
    let tracked_paths: Vec<&str> = tracked.lines().filter(|l| !l.is_empty()).collect();

    for tp in &tracked_paths {
        let bn = Path::new(tp)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if is_denied_name(bn) {
            eprintln!(
                "WARN_COMMITTED_SENSITIVE {tp} (tracked @ pin — still staged; deny-list applies to extras only)"
            );
        }
    }

    fs::create_dir_all(&opts.staging_root).map_err(|e| StageError::Io(e.to_string()))?;
    let staging_root = opts
        .staging_root
        .canonicalize()
        .map_err(|e| StageError::Io(e.to_string()))?;

    let staged_dir = mktemp_under(&staging_root)?;
    extract_archive(&repo, &pin, &staged_dir)?;

    let baseline_dir = if opts.keep_baseline {
        let b = PathBuf::from(format!("{}.baseline", staged_dir.display()));
        fs::create_dir_all(&b).map_err(|e| StageError::Io(e.to_string()))?;
        extract_archive(&repo, &pin, &b)?;
        Some(b)
    } else {
        None
    };

    let manifest_path = PathBuf::from(format!("{}.manifest.jsonl", staged_dir.display()));
    write_manifest(&staged_dir, &tracked_paths, &manifest_path)?;

    Ok(StageResult {
        staged_dir,
        baseline_dir,
        manifest_path,
        pin,
        tracked_count: tracked_paths.len(),
    })
}

/// Write a unified review diff (baseline → staged) for human review / `apply`.
pub fn write_review_diff(
    baseline: &Path,
    staged: &Path,
    out: &Path,
) -> Result<(), StageError> {
    let output = Command::new("diff")
        .args(["-ruN", "--", baseline.to_str().unwrap_or("."), staged.to_str().unwrap_or(".")])
        .output()
        .map_err(|e| StageError::Io(e.to_string()))?;
    // diff exits 1 when files differ — that is success for a review artifact.
    let code = output.status.code().unwrap_or(2);
    if code != 0 && code != 1 {
        return Err(StageError::Io(format!(
            "diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    fs::write(out, &output.stdout).map_err(|e| StageError::Io(e.to_string()))?;
    Ok(())
}

fn mktemp_under(root: &Path) -> Result<PathBuf, StageError> {
    use rand::RngCore;
    for _ in 0..32 {
        let mut b = [0u8; 4];
        rand::thread_rng().fill_bytes(&mut b);
        let name = format!(
            "stage.{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3]
        );
        let path = root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => {
                let canon = path.canonicalize().map_err(|e| StageError::Io(e.to_string()))?;
                if !canon.starts_with(root) {
                    let _ = fs::remove_dir_all(&canon);
                    return Err(StageError::BadRoot(canon.display().to_string()));
                }
                return Ok(canon);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(StageError::Io(e.to_string())),
        }
    }
    Err(StageError::Io("could not allocate staging dir".into()))
}

fn extract_archive(repo: &Path, pin: &str, out: &Path) -> Result<(), StageError> {
    let archive = Command::new("git")
        .args(["-C", repo.to_str().unwrap_or("."), "archive", pin])
        .output()
        .map_err(|e| StageError::Git(e.to_string()))?;
    if !archive.status.success() {
        return Err(StageError::Git(String::from_utf8_lossy(&archive.stderr).into()));
    }
    let mut tar = Command::new("tar")
        .args(["-x", "-C", out.to_str().unwrap_or(".")])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| StageError::Io(e.to_string()))?;
    {
        let mut stdin = tar.stdin.take().ok_or_else(|| StageError::Io("tar stdin".into()))?;
        stdin
            .write_all(&archive.stdout)
            .map_err(|e| StageError::Io(e.to_string()))?;
    }
    let status = tar.wait().map_err(|e| StageError::Io(e.to_string()))?;
    if !status.success() {
        return Err(StageError::Io("tar extract failed".into()));
    }
    // Ensure no .git sneaks in.
    let git_dir = out.join(".git");
    if git_dir.exists() {
        let _ = fs::remove_dir_all(&git_dir);
    }
    Ok(())
}

fn write_manifest(staged: &Path, tracked: &[&str], manifest: &Path) -> Result<(), StageError> {
    let mut f = fs::File::create(manifest).map_err(|e| StageError::Io(e.to_string()))?;
    for tp in tracked {
        let path = staged.join(tp);
        if path.is_file() {
            let bytes = fs::read(&path).map_err(|e| StageError::Io(e.to_string()))?;
            let hash = hex_sha256(&bytes);
            writeln!(
                f,
                r#"{{"path":{},"type":"tracked","sha256":"{}"}}"#,
                serde_json::to_string(tp).unwrap_or_else(|_| format!("\"{tp}\"")),
                hash
            )
            .map_err(|e| StageError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<String, StageError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().map_err(|e| StageError::Git(e.to_string()))?;
    if !out.status.success() {
        return Err(StageError::Git(String::from_utf8_lossy(&out.stderr).trim().into()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo(dir: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(dir)
            .status()
            .unwrap();
        fs::write(dir.join("README.md"), "ok\n").unwrap();
        fs::write(dir.join(".env"), "secret\n").unwrap(); // untracked
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .status()
            .unwrap();
    }

    #[test]
    fn stages_tracked_without_git_or_untracked_env() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let root = tmp.path().join("staging");
        let res = stage_repo(&StageOptions {
            repo,
            rev: "HEAD".into(),
            staging_root: root,
            keep_baseline: true,
        })
        .unwrap();
        assert!(res.staged_dir.join("README.md").is_file());
        assert!(!res.staged_dir.join(".git").exists());
        assert!(!res.staged_dir.join(".env").exists());
        assert!(res.manifest_path.is_file());
        assert!(res.baseline_dir.unwrap().join("README.md").is_file());
        assert!(res.tracked_count >= 1);
    }
}
