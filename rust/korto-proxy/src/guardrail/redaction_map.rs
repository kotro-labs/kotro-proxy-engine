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

/// Restores redacted placeholders inside an SSE data payload byte slice.
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
