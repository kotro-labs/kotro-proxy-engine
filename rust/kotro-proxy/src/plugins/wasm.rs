use anyhow::{Context, Result};
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

    /// Executes `on_request` on all loaded plugins in sequence.
    /// If any plugin returns `block: true`, the chain stops and returns immediately.
    /// Otherwise, modifications to the request body are passed to the next plugin.
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
            let mut plugin = match plugin_mutex.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    tracing::error!("Failed to lock plugin mutex: {}", e);
                    continue;
                }
            };
            
            if !plugin.function_exists("on_request") {
                continue;
            }

            let input_json = serde_json::to_string(&current_req)?;
            match plugin.call::<&str, &str>("on_request", &input_json) {
                Ok(output_json) => {
                    match serde_json::from_str::<WasmResponse>(output_json) {
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
                Err(e) => {
                    tracing::warn!("WASM plugin execution error in on_request: {}", e);
                }
            }
        }
        
        Ok(final_res)
    }
}
