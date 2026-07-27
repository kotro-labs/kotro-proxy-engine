//! Action-plane reporting client: mcp-wrap / hook adapters push events into
//! the proxy's flight recorder over the authenticated control API, and read
//! kill-switch / session-label / approval state back.
//!
//! Fully fail-open for *telemetry* (events fall back to a local JSONL file
//! when the proxy is down) but fail-closed for *enforcement reads* the caller
//! treats as such (kill switch defaults to "not engaged" only because a dead
//! proxy must not brick every tool call; policy still applies locally).

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::flight_recorder::KillScope;
use crate::router::control_auth::CONTROL_TOKEN_FILE;

#[derive(Clone)]
pub struct Reporter {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
    fallback_path: PathBuf,
    pub session: String,
}

impl Reporter {
    pub fn new(state_dir: &std::path::Path, session: String) -> Self {
        let metrics_addr =
            std::env::var("KOTRO_METRICS_ADDR").unwrap_or_else(|_| "127.0.0.1:9090".into());
        let host = if metrics_addr.starts_with(':') {
            format!("127.0.0.1{metrics_addr}")
        } else {
            metrics_addr
        };
        let token = std::env::var("KOTRO_CONTROL_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                std::fs::read_to_string(state_dir.join(CONTROL_TOKEN_FILE))
                    .ok()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
            });
        Self {
            base_url: format!("http://{host}"),
            token,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default(),
            fallback_path: state_dir.join("mcp-events.jsonl"),
            session,
        }
    }

    /// Fire-and-forget event report. `draft` is a `FlightDraft`-shaped object.
    pub fn report(&self, mut draft: Value) {
        if let Some(obj) = draft.as_object_mut() {
            obj.entry("session").or_insert(Value::String(self.session.clone()));
        }
        let this = self.clone();
        tokio::spawn(async move {
            let mut req = this
                .client
                .post(format!("{}/api/mcp-event", this.base_url))
                .json(&draft);
            if let Some(token) = &this.token {
                req = req.header("x-kotro-control-token", token);
            }
            let ok = matches!(req.send().await, Ok(resp) if resp.status().is_success());
            if !ok {
                // Local JSONL fallback so the tape survives a proxy outage.
                if let Some(parent) = this.fallback_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&this.fallback_path)
                {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            &this.fallback_path,
                            std::fs::Permissions::from_mode(0o600),
                        );
                    }
                    use std::io::Write;
                    let _ = writeln!(f, "{draft}");
                }
            }
        });
    }

    /// Current kill-switch scope from the proxy. `None` when unreachable.
    pub async fn kill_scope(&self) -> Option<KillScope> {
        let resp = self
            .client
            .get(format!("{}/api/kill-switch", self.base_url))
            .send()
            .await
            .ok()?;
        let v: Value = resp.json().await.ok()?;
        v.get("scope")
            .and_then(Value::as_str)
            .map(KillScope::parse)
    }

    /// Provenance labels the correlation engine has attached to this session.
    pub async fn session_labels(&self) -> Vec<String> {
        let Ok(resp) = self
            .client
            .get(format!(
                "{}/api/session-labels?session={}",
                self.base_url,
                urlencode(&self.session)
            ))
            .send()
            .await
        else {
            return Vec::new();
        };
        let Ok(v) = resp.json::<Value>().await else {
            return Vec::new();
        };
        v.get("labels")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Check whether an approval grant exists for this exact call shape. When
    /// `reason` is non-empty and no grant exists, the proxy queues this call
    /// as a pending approval for the local approval UX.
    pub async fn check_approval(
        &self,
        server: &str,
        tool: &str,
        args_hash: &str,
        reason: &str,
    ) -> bool {
        let mut req = self.client.get(format!(
            "{}/api/approvals/check?server={}&tool={}&args_hash={}&session={}&reason={}",
            self.base_url,
            urlencode(server),
            urlencode(tool),
            urlencode(args_hash),
            urlencode(&self.session),
            urlencode(reason),
        ));
        if let Some(token) = &self.token {
            req = req.header("x-kotro-control-token", token);
        }
        let Ok(resp) = req.send().await else {
            return false;
        };
        resp.json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("granted").and_then(Value::as_bool))
            .unwrap_or(false)
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_basic() {
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
        assert_eq!(urlencode("safe-string_1.0~x"), "safe-string_1.0~x");
    }
}
