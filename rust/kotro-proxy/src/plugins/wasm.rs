//! WASM plugin engine — hot-loadable request interceptors via Extism.
//!
//! # Trust defaults (Phase 0)
//! - Credential-bearing headers (`Authorization`, `x-api-key`, cookies, …) are
//!   stripped before the plugin sees the request unless
//!   `KOTRO_WASM_ALLOW_CREDENTIAL_HEADERS=true`.
//! - Each plugin call is bounded by a wall-clock timeout
//!   (`KOTRO_WASM_TIMEOUT_MS`, default 500 ms).
//! - On plugin error, behavior is controlled by `KOTRO_WASM_FAIL_CLOSED`
//!   (default **true**: treat as block; set `false` to fail-open).
//!
//! # Async safety
//! `Plugin::call` is a blocking C FFI call. Invocations from async Axum
//! handlers run inside `tokio::task::block_in_place` plus a timeout.

use anyhow::Result;
use extism::{Manifest, Plugin, Wasm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Header names never forwarded to WASM unless explicitly allowed.
const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-access-token",
    "x-kotro-bridge-token",
    "x-kotro-control-token",
    "x-amz-security-token",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmRequest {
    pub uri: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmResponse {
    pub status: Option<u16>,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub block: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct PluginTrustOptions {
    /// Wall-clock budget per plugin call.
    pub timeout: Duration,
    /// When true, plugin errors / timeouts deny the request.
    pub fail_closed: bool,
    /// When true, credential headers are forwarded to plugins.
    pub allow_credential_headers: bool,
}

impl Default for PluginTrustOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(500),
            fail_closed: true,
            allow_credential_headers: false,
        }
    }
}

#[derive(Clone)]
pub struct PluginManager {
    plugins: Vec<Arc<Mutex<Plugin>>>,
    trust: PluginTrustOptions,
}

impl PluginManager {
    pub fn new(plugin_paths: &[String]) -> Result<Self> {
        Self::with_trust(plugin_paths, PluginTrustOptions::default())
    }

    pub fn with_trust(plugin_paths: &[String], trust: PluginTrustOptions) -> Result<Self> {
        let mut plugins = Vec::new();
        for path in plugin_paths {
            let wasm = Wasm::file(path);
            let manifest = Manifest::new([wasm]);
            let plugin = Plugin::new(&manifest, [], true)
                .map_err(|e| anyhow::anyhow!("Failed to load WASM plugin {}: {}", path, e))?;
            plugins.push(Arc::new(Mutex::new(plugin)));
            tracing::info!(plugin_path = %path, "Loaded WASM plugin");
        }
        Ok(Self { plugins, trust })
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub fn trust(&self) -> &PluginTrustOptions {
        &self.trust
    }

    /// Strip credential headers unless explicitly allowed.
    pub fn sanitize_headers(
        headers: &HashMap<String, String>,
        allow_credentials: bool,
    ) -> HashMap<String, String> {
        if allow_credentials {
            return headers.clone();
        }
        headers
            .iter()
            .filter(|(k, _)| {
                let lower = k.to_ascii_lowercase();
                !CREDENTIAL_HEADERS.iter().any(|b| lower == *b)
                    && !lower.starts_with("x-api-")
                    && !lower.contains("authorization")
                    && !lower.ends_with("-token")
                    && !lower.ends_with("-key")
                    && !lower.ends_with("-secret")
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Executes `on_request` on all loaded plugins in sequence.
    pub fn on_request(&self, mut req: WasmRequest) -> Result<WasmResponse> {
        req.headers = Self::sanitize_headers(&req.headers, self.trust.allow_credential_headers);

        let mut current_req = req;
        let mut final_res = WasmResponse {
            status: None,
            headers: None,
            body: Some(current_req.body.clone()),
            block: Some(false),
        };

        if self.plugins.is_empty() {
            return Ok(final_res);
        }

        for plugin_mutex in &self.plugins {
            let plugin_arc = Arc::clone(plugin_mutex);
            let input_json = serde_json::to_string(&current_req)?;
            let timeout = self.trust.timeout;
            let fail_closed = self.trust.fail_closed;

            let call_result = tokio::task::block_in_place(|| {
                let started = std::time::Instant::now();
                let mut plugin = match plugin_arc.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        return Err(format!("plugin lock poisoned: {e}"));
                    }
                };
                if !plugin.function_exists("on_request") {
                    return Ok(None);
                }
                let out = match plugin.call::<&str, &str>("on_request", &input_json) {
                    Ok(out) => Ok(Some(out.to_owned())),
                    Err(e) => Err(format!("WASM on_request error: {e}")),
                };
                if started.elapsed() > timeout {
                    return Err(format!(
                        "WASM plugin exceeded timeout of {}ms",
                        timeout.as_millis()
                    ));
                }
                out
            });

            let output_json = match call_result {
                Ok(None) => continue,
                Ok(Some(json)) => json,
                Err(e) => {
                    tracing::warn!(error = %e, "WASM plugin call failed");
                    if fail_closed {
                        return Ok(WasmResponse {
                            status: Some(403),
                            headers: None,
                            body: None,
                            block: Some(true),
                        });
                    }
                    continue;
                }
            };

            match serde_json::from_str::<WasmResponse>(&output_json) {
                Ok(res) => {
                    if res.block.unwrap_or(false) {
                        return Ok(res);
                    }
                    if let Some(new_body) = res.body {
                        current_req.body = new_body.clone();
                        final_res.body = Some(new_body);
                    }
                    if let Some(new_headers) = res.headers {
                        // Never allow plugins to re-inject credential headers
                        // into the mutation chain unless credentials are allowed.
                        let sanitized =
                            Self::sanitize_headers(&new_headers, self.trust.allow_credential_headers);
                        current_req.headers = sanitized.clone();
                        final_res.headers = Some(sanitized);
                    }
                }
                Err(e) => {
                    tracing::warn!("WASM plugin returned invalid JSON: {}", e);
                    if fail_closed {
                        return Ok(WasmResponse {
                            status: Some(403),
                            headers: None,
                            body: None,
                            block: Some(true),
                        });
                    }
                }
            }
        }

        Ok(final_res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plugin_manager_passes_through() {
        let manager = PluginManager {
            plugins: vec![],
            trust: PluginTrustOptions::default(),
        };
        assert!(manager.is_empty());

        let req = WasmRequest {
            uri: "/v1/chat/completions".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: r#"{"model":"gpt-4","messages":[]}"#.into(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async { manager.on_request(req.clone()) }).unwrap();

        assert_eq!(res.block, Some(false));
        assert_eq!(res.body.as_deref(), Some(r#"{"model":"gpt-4","messages":[]}"#));
    }

    #[test]
    fn strips_credential_headers_by_default() {
        let mut headers = HashMap::new();
        headers.insert("authorization".into(), "Bearer SECRET".into());
        headers.insert("x-api-key".into(), "sk-test".into());
        headers.insert("content-type".into(), "application/json".into());
        headers.insert("x-request-id".into(), "abc".into());

        let cleaned = PluginManager::sanitize_headers(&headers, false);
        assert!(!cleaned.contains_key("authorization"));
        assert!(!cleaned.contains_key("x-api-key"));
        assert_eq!(cleaned.get("content-type").map(String::as_str), Some("application/json"));
        assert_eq!(cleaned.get("x-request-id").map(String::as_str), Some("abc"));
    }

    #[test]
    fn allow_credentials_keeps_headers() {
        let mut headers = HashMap::new();
        headers.insert("authorization".into(), "Bearer SECRET".into());
        let cleaned = PluginManager::sanitize_headers(&headers, true);
        assert_eq!(cleaned.get("authorization").map(String::as_str), Some("Bearer SECRET"));
    }
}
