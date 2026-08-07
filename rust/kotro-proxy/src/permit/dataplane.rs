//! Dual-home data-plane sidecar — agent reaches Kotro plane only; tokens stay off agent.

use std::process::Command;

use thiserror::Error;

use crate::permit::docker::{docker_ok_pub, sanitize_pub, DockerError};

#[derive(Debug, Error)]
pub enum DataplaneError {
    #[error("dataplane: {0}")]
    Msg(String),
}

impl From<DockerError> for DataplaneError {
    fn from(e: DockerError) -> Self {
        DataplaneError::Msg(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct DualHomeNets {
    pub agent_net: String,
    pub up_net: String,
}

#[derive(Debug, Clone)]
pub struct DataplaneHandle {
    pub container_name: String,
    /// IP on the **agent** network (what the agent should dial).
    pub agent_ip: String,
    pub listen_port: u16,
    pub nets: DualHomeNets,
}

/// Create agent (`--internal`) + upstream (egress-capable) networks.
pub fn create_dual_home_nets(run_id: &str) -> Result<DualHomeNets, DataplaneError> {
    let agent_net = format!("kotro-permit-{}-agent", sanitize_pub(run_id));
    let up_net = format!("kotro-permit-{}-up", sanitize_pub(run_id));
    let _ = docker_ok_pub(&["network", "rm", &agent_net]);
    let _ = docker_ok_pub(&["network", "rm", &up_net]);
    docker_ok_pub(&[
        "network",
        "create",
        "--internal",
        "--label",
        "kotro.permit=agent",
        &agent_net,
    ])?;
    docker_ok_pub(&["network", "create", "--label", "kotro.permit=up", &up_net])?;
    Ok(DualHomeNets { agent_net, up_net })
}

pub fn remove_dual_home_nets(nets: &DualHomeNets) {
    let _ = docker_ok_pub(&["network", "rm", &nets.agent_net]);
    let _ = docker_ok_pub(&["network", "rm", &nets.up_net]);
}

/// Start a dual-homed data-plane container.
///
/// - Joined to **agent** (internal) and **up** networks.
/// - Holds `PROVIDER_TOKEN` / upstream URL in **its** env only.
/// - Serves a minimal `/v1/*` mediator; `/control` is denied.
/// - `upstream_url`: if set, mediates GET to that URL; else returns local OK body
///   (dogfood without a live host proxy).
pub fn start_dataplane(
    run_id: &str,
    nets: &DualHomeNets,
    image: &str,
    upstream_url: Option<&str>,
    provider_token: Option<&str>,
    broker_forward: Option<&str>,
) -> Result<DataplaneHandle, DataplaneError> {
    let name = format!("kotro-dp-{}", sanitize_pub(run_id));
    let _ = Command::new("docker").args(["rm", "-f", &name]).status();

    let tok = provider_token.unwrap_or("host-only-provider-token");
    let up = upstream_url.unwrap_or("");
    let fwd = broker_forward.unwrap_or("");

    let py = r#"
import os, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer
UP=os.environ.get("KOTRO_UPSTREAM","").strip()
TOK=os.environ.get("PROVIDER_TOKEN","")
FWD=os.environ.get("KOTRO_BROKER_FORWARD","").strip().rstrip("/")
class H(BaseHTTPRequestHandler):
  def _read(self):
    n=int(self.headers.get("Content-Length") or 0)
    return self.rfile.read(n) if n else b""
  def do_GET(self):
    if self.path.startswith("/control"):
      self.send_response(403); self.end_headers(); self.wfile.write(b"CONTROL_DENIED"); return
    if self.path.startswith("/health"):
      self.send_response(200); self.end_headers(); self.wfile.write(b"DATAPLANE_OK"); return
    if self.path.startswith("/v1/"):
      if UP:
        try:
          req=urllib.request.Request(UP, headers={"Authorization":"Bearer "+TOK} if TOK else {})
          body=urllib.request.urlopen(req, timeout=5).read()
          self.send_response(200); self.end_headers()
          self.wfile.write(b"DATAPLANE_OK "+body[:200]); return
        except Exception as e:
          self.send_response(502); self.end_headers(); self.wfile.write(str(e).encode()); return
      self.send_response(200); self.end_headers(); self.wfile.write(b"DATAPLANE_OK local"); return
    self.send_response(404); self.end_headers()
  def do_POST(self):
    if self.path.startswith("/v1/broker/") and FWD:
      try:
        data=self._read()
        req=urllib.request.Request(
          FWD+self.path,
          data=data,
          headers={
            "Content-Type": self.headers.get("Content-Type") or "application/json",
            "Authorization": self.headers.get("Authorization") or "",
          },
          method="POST",
        )
        with urllib.request.urlopen(req, timeout=120) as resp:
          body=resp.read()
          self.send_response(resp.status); self.end_headers(); self.wfile.write(body)
      except Exception as e:
        code=502
        msg=str(e).encode()
        if hasattr(e, "code"):
          code=int(e.code)
          try: msg=e.read()
          except Exception: pass
        self.send_response(code); self.end_headers(); self.wfile.write(msg)
      return
    self.send_response(404); self.end_headers()
  def log_message(self,*a): pass
HTTPServer(("0.0.0.0",8080),H).serve_forever()
"#;

    let status = Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            &name,
            "--network",
            &nets.agent_net,
            "--label",
            "kotro.permit=dataplane",
            "--add-host",
            "host.docker.internal:host-gateway",
            "-e",
            &format!("PROVIDER_TOKEN={tok}"),
            "-e",
            &format!("KOTRO_UPSTREAM={up}"),
            "-e",
            &format!("KOTRO_BROKER_FORWARD={fwd}"),
            image,
            "python",
            "-c",
            py,
        ])
        .status()
        .map_err(|e| DataplaneError::Msg(e.to_string()))?;
    if !status.success() {
        return Err(DataplaneError::Msg(
            "failed to start dataplane container (need python image?)".into(),
        ));
    }

    // Attach upstream network so dataplane can reach host/upstream; agent cannot.
    docker_ok_pub(&["network", "connect", &nets.up_net, &name])?;

    // Resolve agent-net IP.
    let insp = Command::new("docker")
        .args([
            "inspect",
            "-f",
            &format!(
                "{{{{(index .NetworkSettings.Networks \"{}\").IPAddress}}}}",
                nets.agent_net
            ),
            &name,
        ])
        .output()
        .map_err(|e| DataplaneError::Msg(e.to_string()))?;
    if !insp.status.success() {
        let _ = Command::new("docker").args(["rm", "-f", &name]).status();
        return Err(DataplaneError::Msg("inspect dataplane IP failed".into()));
    }
    let agent_ip = String::from_utf8_lossy(&insp.stdout).trim().to_string();
    if agent_ip.is_empty() {
        let _ = Command::new("docker").args(["rm", "-f", &name]).status();
        return Err(DataplaneError::Msg("empty dataplane agent IP".into()));
    }

    Ok(DataplaneHandle {
        container_name: name,
        agent_ip,
        listen_port: 8080,
        nets: nets.clone(),
    })
}

pub fn stop_dataplane(handle: &DataplaneHandle) {
    let _ = Command::new("docker")
        .args(["rm", "-f", &handle.container_name])
        .status();
    remove_dual_home_nets(&handle.nets);
}

impl DataplaneHandle {
    pub fn broker_url(&self) -> String {
        format!("http://{}:{}", self.agent_ip, self.listen_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_run_id_safe() {
        let s = sanitize_pub("run-abc/../x");
        assert!(!s.contains('/'));
        assert!(!s.contains('.'));
    }
}
