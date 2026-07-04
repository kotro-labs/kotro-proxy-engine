//! Per-request placeholder registry — mirrors `internal/guardrail/redactor.go`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

#[derive(Debug, Default)]
pub struct RedactionMap {
    forward: RwLock<HashMap<String, String>>,
    reverse: RwLock<HashMap<String, String>>,
    seq: AtomicUsize,
}

impl RedactionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.read().unwrap().is_empty()
    }

    pub fn len(&self) -> usize {
        self.forward.read().unwrap().len()
    }

    pub fn placeholder_for(&self, original: &str) -> String {
        if let Some(ph) = self.reverse.read().unwrap().get(original) {
            return ph.clone();
        }
        let n = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let placeholder = format!("[REDACTED_SECRET_{n}]");
        self.forward
            .write()
            .unwrap()
            .insert(placeholder.clone(), original.to_string());
        self.reverse
            .write()
            .unwrap()
            .insert(original.to_string(), placeholder.clone());
        placeholder
    }

    /// Registers a placeholder → original mapping (test / pipeline hooks).
    pub fn insert(&self, placeholder: impl Into<String>, original: impl Into<String>) {
        let placeholder = placeholder.into();
        let original = original.into();
        self.forward
            .write()
            .unwrap()
            .insert(placeholder.clone(), original.clone());
        self.reverse.write().unwrap().insert(original, placeholder);
    }

    /// Reverses placeholder masking on inbound streaming text.
    pub fn restore(&self, text: &str) -> String {
        let map = self.forward.read().unwrap();
        if map.is_empty() {
            return text.to_string();
        }
        let mut result = text.to_string();
        for (placeholder, original) in map.iter() {
            result = result.replace(placeholder, original);
        }
        result
    }
}

/// Restores redacted placeholders inside an SSE data payload byte slice based on provider format.
pub fn restore_payload_counted(
    payload: &[u8],
    map: &RedactionMap,
    format: crate::proxy::StreamFormat,
) -> (Vec<u8>, usize) {
    if map.is_empty() {
        return (payload.to_vec(), 0);
    }
    match format {
        crate::proxy::StreamFormat::OpenAI => restore_openai_chunk(payload, map),
        crate::proxy::StreamFormat::Anthropic => restore_anthropic_delta(payload, map),
    }
}

fn restore_openai_chunk(payload: &[u8], map: &RedactionMap) -> (Vec<u8>, usize) {
    let mut val: serde_json::Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(_) => return (payload.to_vec(), 0),
    };

    let mut restores = 0;
    if let Some(choices) = val.get_mut("choices").and_then(|c| c.as_array_mut()) {
        for choice in choices {
            if let Some(delta) = choice.get_mut("delta").and_then(|d| d.as_object_mut()) {
                if let Some(content_val) = delta.get_mut("content") {
                    if let Some(content_str) = content_val.as_str() {
                        if !content_str.is_empty() {
                            let (restored, count) = restore_counted(content_str, map);
                            *content_val = serde_json::Value::String(restored);
                            restores += count;
                        }
                    }
                }
            }
        }
    }

    let out = serde_json::to_vec(&val).unwrap_or_else(|_| payload.to_vec());
    (out, restores)
}

fn restore_anthropic_delta(payload: &[u8], map: &RedactionMap) -> (Vec<u8>, usize) {
    let mut val: serde_json::Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(_) => return (payload.to_vec(), 0),
    };

    let mut restores = 0;
    if val.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
        if let Some(delta) = val.get_mut("delta").and_then(|d| d.as_object_mut()) {
            if let Some(text_val) = delta.get_mut("text") {
                if let Some(text_str) = text_val.as_str() {
                    if !text_str.is_empty() {
                        let (restored, count) = restore_counted(text_str, map);
                        *text_val = serde_json::Value::String(restored);
                        restores += count;
                    }
                }
            }
        }
    }

    let out = serde_json::to_vec(&val).unwrap_or_else(|_| payload.to_vec());
    (out, restores)
}

fn restore_counted(text: &str, map: &RedactionMap) -> (String, usize) {
    let forward = map.forward.read().unwrap();
    if forward.is_empty() {
        return (text.to_string(), 0);
    }
    let mut result = text.to_string();
    let mut restores = 0;
    for (placeholder, original) in forward.iter() {
        if result.contains(placeholder) {
            let count = result.matches(placeholder).count();
            restores += count;
            result = result.replace(placeholder, original);
        }
    }
    (result, restores)
}

/// Restores redacted placeholders inside an SSE data payload byte slice (legacy).
pub fn restore_payload(payload: &[u8], map: &RedactionMap) -> Vec<u8> {
    if map.is_empty() {
        return payload.to_vec();
    }
    let Ok(text) = std::str::from_utf8(payload) else {
        return payload.to_vec();
    };
    map.restore(text).into_bytes()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_reverses_placeholders() {
        let map = RedactionMap::new();
        map.insert("[REDACTED_SECRET_1]", "AKIAIOSFODNN7EXAMPLE");
        let out = map.restore(r#"{"content":"[REDACTED_SECRET_1]"}"#);
        assert!(out.contains("AKIAIOSFODNN7EXAMPLE"));
    }
}
