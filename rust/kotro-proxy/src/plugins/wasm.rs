//! WASM plugin engine — hot-loadable request/response interceptors via Extism.
//!
//! # Async safety
//! `Plugin::call` is a blocking C FFI call into the WASM runtime. To avoid
//! stalling the Tokio executor when plugins are invoked from async Axum handlers,
//! `on_request` wraps each plugin call in `tokio::task::block_in_place`.
//!
//! # Plugin chain
//! Plugins execute in load order. Each plugin receives the (possibly modified)
//! request body from the previous plugin. If any plugin sets `block: true`,
//! the chain short-circuits and the request is rejected immediately.

use anyhow::Result;
use extism::{Manifest, Plugin, Wasm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
pub struct PluginManager {
    plugins: Vec<Arc<Mutex<Plugin>>>,
}

impl PluginManager {
    pub fn new(plugin_paths: &[String]) -> Result<Self> {
        let mut plugins = Vec::new();
        for path in plugin_paths {
            let wasm = Wasm::file(path);
            let manifest = Manifest::new([wasm]);
            let plugin = Plugin::new(&manifest, [], true)
                .map_err(|e| anyhow::anyhow!("Failed to load WASM plugin {}: {}", path, e))?;
            plugins.push(Arc::new(Mutex::new(plugin)));
            tracing::info!(plugin_path = %path, "Loaded WASM plugin");
        }
        Ok(Self { plugins })
    }

    /// Returns `true` if no plugins are loaded (fast-path check for handlers).
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Executes `on_request` on all loaded plugins in sequence.
    ///
    /// Each plugin call runs inside `block_in_place` so the Tokio thread pool
    /// is not starved while the WASM runtime executes. If any plugin returns
    /// `block: true`, the chain stops immediately. Modifications to the request
    /// body propagate to the next plugin in the chain.
    pub fn on_request(&self, req: WasmRequest) -> Result<WasmResponse> {
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

            // block_in_place: the WASM FFI call may take non-trivial time.
            // This yields the executor while we hold the OS thread.
            let output = tokio::task::block_in_place(|| {
                let mut plugin = match plugin_arc.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        tracing::error!("Failed to lock plugin mutex: {}", e);
                        return None;
                    }
                };

                if !plugin.function_exists("on_request") {
                    return None;
                }

                match plugin.call::<&str, &str>("on_request", &input_json) {
                    Ok(out) => Some(out.to_owned()),
                    Err(e) => {
                        tracing::warn!("WASM plugin execution error in on_request: {}", e);
                        None
                    }
                }
            });

            let Some(output_json) = output else {
                continue;
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
                        current_req.headers = new_headers.clone();
                        final_res.headers = Some(new_headers);
                    }
                }
                Err(e) => {
                    tracing::warn!("WASM plugin returned invalid JSON: {}", e);
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
        let manager = PluginManager { plugins: vec![] };
        assert!(manager.is_empty());

        // on_request with no plugins returns the original body unchanged.
        let req = WasmRequest {
            uri: "/v1/chat/completions".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: r#"{"model":"gpt-4","messages":[]}"#.into(),
        };

        // We need a tokio runtime for block_in_place
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async { manager.on_request(req.clone()) }).unwrap();

        assert_eq!(res.block, Some(false));
        assert_eq!(res.body.as_deref(), Some(r#"{"model":"gpt-4","messages":[]}"#));
    }
}
