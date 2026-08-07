//! Docker sandbox backend — start agent confinement or refuse (never host exec).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

use crate::permit::sandbox::{sandbox_backend_available, SandboxStatus};

#[derive(Debug, Error)]
pub enum DockerError {
    #[error("sandbox unavailable: {0}")]
    Unavailable(String),
    #[error("docker: {0}")]
    Docker(String),
}

#[derive(Debug, Clone)]
pub struct DockerRunOptions {
    pub run_id: String,
    pub image: String,
    pub workspace: PathBuf,
    /// Working directory inside the container.
    pub workdir: String,
    pub agent_cmd: Vec<String>,
    pub env: HashMap<String, String>,
    pub memory: String,
    pub cpus: String,
    pub pids_limit: String,
    /// When true, use `--network none` (stronger than internal for alpha isolation tests).
    pub network_none: bool,
    /// Pre-created agent network name (dual-home). When set, skip create/rm here.
    pub agent_network: Option<String>,
    /// When false, caller owns network lifecycle (dataplane teardown).
    pub cleanup_network: bool,
}

#[derive(Debug)]
pub struct DockerRunResult {
    pub exit_code: i32,
    pub network: String,
    pub container_name: String,
}

/// Env keys that must never enter the agent container.
const BLOCKED_ENV: &[&str] = &[
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "DEEPSEEK_API_KEY",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_ACCESS_KEY_ID",
    "KOTRO_UPSTREAM_API_KEY",
    "KOTRO_BRIDGE_TOKEN",
];

pub fn assert_sandbox_ready() -> Result<SandboxStatus, DockerError> {
    match sandbox_backend_available() {
        s @ SandboxStatus::Available { .. } => Ok(s),
        SandboxStatus::Unavailable { reason } => Err(DockerError::Unavailable(reason)),
    }
}

/// Create an internal Docker network for this run (public egress blocked as baseline).
pub fn create_internal_network(run_id: &str) -> Result<String, DockerError> {
    let name = format!("kotro-permit-{}", sanitize(run_id));
    let _ = docker_ok(&["network", "rm", &name]); // best-effort cleanup leftover
    docker_ok(&[
        "network",
        "create",
        "--internal",
        "--label",
        "kotro.permit=1",
        &name,
    ])?;
    Ok(name)
}

pub fn remove_network(name: &str) -> Result<(), DockerError> {
    let _ = docker_ok(&["network", "rm", name]);
    Ok(())
}

pub fn docker_ok_pub(args: &[&str]) -> Result<(), DockerError> {
    docker_ok(args)
}

pub fn sanitize_pub(s: &str) -> String {
    sanitize(s)
}

/// Run the agent command inside a one-shot container. Never runs on the host.
pub fn run_agent_container(opts: &DockerRunOptions) -> Result<DockerRunResult, DockerError> {
    assert_sandbox_ready()?;

    let (network, owned) = if opts.network_none {
        ("none".to_string(), false)
    } else if let Some(n) = &opts.agent_network {
        (n.clone(), false)
    } else {
        (create_internal_network(&opts.run_id)?, true)
    };
    let cleanup = opts.cleanup_network && owned;

    let container_name = format!("kotro-agent-{}", sanitize(&opts.run_id));
    let ws = opts
        .workspace
        .canonicalize()
        .map_err(|e| DockerError::Docker(e.to_string()))?;

    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        container_name.clone(),
        "--network".into(),
        network.clone(),
        "--memory".into(),
        opts.memory.clone(),
        "--cpus".into(),
        opts.cpus.clone(),
        "--pids-limit".into(),
        opts.pids_limit.clone(),
        "--tmpfs".into(),
        "/tmp:rw,noexec,nosuid,size=64m".into(),
        "--tmpfs".into(),
        "/home/agent:rw,nosuid,size=16m".into(),
        "-e".into(),
        "HOME=/home/agent".into(),
        "-v".into(),
        format!("{}:/workspace:rw", ws.display()),
        "-w".into(),
        opts.workdir.clone(),
    ];

    // Inject allowlisted env only (explicit map — never inherit host secrets).
    for (k, v) in &opts.env {
        if BLOCKED_ENV.iter().any(|b| *b == k.as_str()) {
            continue;
        }
        if k.contains("TOKEN") && k != "KOTRO_RUN_TOKEN" {
            continue;
        }
        if k.contains("API_KEY") || k.contains("SECRET") {
            continue;
        }
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }

    args.push(opts.image.clone());
    if opts.agent_cmd.is_empty() {
        if cleanup {
            let _ = remove_network_if_ours(&network);
        }
        return Err(DockerError::Docker("empty agent command".into()));
    }
    // Use sh -c when a single shell string isn't provided — pass argv directly.
    for a in &opts.agent_cmd {
        args.push(a.clone());
    }

    let status = Command::new("docker")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| DockerError::Docker(e.to_string()))?;

    if cleanup {
        let _ = remove_network(&network);
    }

    Ok(DockerRunResult {
        exit_code: status.code().unwrap_or(1),
        network,
        container_name,
    })
}

fn remove_network_if_ours(network: &str) {
    if network != "none" {
        let _ = remove_network(network);
    }
}

fn docker_ok(args: &[&str]) -> Result<(), DockerError> {
    let out = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| DockerError::Docker(e.to_string()))?;
    if !out.status.success() {
        return Err(DockerError::Docker(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .take(48)
        .collect()
}

/// Confirm a path is not docker.sock / secret dir before any mount wiring.
pub fn refuse_dangerous_mount(path: &Path) -> Result<(), DockerError> {
    let s = path.to_string_lossy();
    if s.contains("docker.sock") || s.ends_with("/.ssh") || s.contains("/.ssh/") {
        return Err(DockerError::Docker(format!(
            "refusing dangerous mount: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_secret_mounts() {
        assert!(refuse_dangerous_mount(Path::new("/var/run/docker.sock")).is_err());
        assert!(refuse_dangerous_mount(Path::new("/Users/x/.ssh")).is_err());
        assert!(refuse_dangerous_mount(Path::new("/tmp/stage.abc")).is_ok());
    }

    #[test]
    fn blocked_env_list_covers_github_and_providers() {
        assert!(BLOCKED_ENV.contains(&"GITHUB_TOKEN"));
        assert!(BLOCKED_ENV.contains(&"ANTHROPIC_API_KEY"));
    }
}
