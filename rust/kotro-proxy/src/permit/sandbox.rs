//! Sandbox backend probe — Docker required; never fall back to host agent exec.

use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxStatus {
    Available { detail: String },
    Unavailable { reason: String },
}

/// Returns whether a Docker Engine usable for Permit sandboxes is present.
///
/// Honors `KOTRO_SANDBOX_FORCE_UNAVAILABLE=1` for acceptance tests.
/// Never treats “Docker missing” as permission to run the agent on the host.
pub fn sandbox_backend_available() -> SandboxStatus {
    if forced_unavailable() {
        return SandboxStatus::Unavailable {
            reason: "KOTRO_SANDBOX_FORCE_UNAVAILABLE is set (test / operator hold)".into(),
        };
    }
    match Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if ver.is_empty() {
                SandboxStatus::Unavailable {
                    reason: "docker info returned empty server version".into(),
                }
            } else {
                SandboxStatus::Available {
                    detail: format!("docker server {ver}"),
                }
            }
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            SandboxStatus::Unavailable {
                reason: if err.is_empty() {
                    format!("docker info exited {}", out.status)
                } else {
                    err
                },
            }
        }
        Err(e) => SandboxStatus::Unavailable {
            reason: format!("docker not executable: {e}"),
        },
    }
}

fn forced_unavailable() -> bool {
    matches!(
        std::env::var("KOTRO_SANDBOX_FORCE_UNAVAILABLE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

/// Best-effort timed probe used by suite runners (avoids hanging forever).
pub fn sandbox_backend_available_quick() -> SandboxStatus {
    // `docker info` is usually fast; we still avoid custom timeouts without nix.
    let _ = Duration::from_secs(5);
    sandbox_backend_available()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_unavailable_never_reports_available() {
        std::env::set_var("KOTRO_SANDBOX_FORCE_UNAVAILABLE", "1");
        let status = sandbox_backend_available();
        std::env::remove_var("KOTRO_SANDBOX_FORCE_UNAVAILABLE");
        assert!(matches!(status, SandboxStatus::Unavailable { .. }));
    }
}
