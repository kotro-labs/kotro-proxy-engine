//! Semantic cache keying — mirrors `internal/cache/semantic.go`.

use sha2::{Digest, Sha256};

fn semantic_key(system_prompt: &str, latest_user: &str) -> String {
    let mut h = Sha256::new();
    h.update(system_prompt.as_bytes());
    h.update([0u8]);
    h.update(latest_user.as_bytes());
    hex_encode(h.finalize().as_slice())
}

/// Hashes prompt state, model, and provider namespace for lookup.
pub fn key_for_request(
    system_prompt: &str,
    latest_user: &str,
    model: &str,
    provider: &str,
) -> String {
    let mut base = semantic_key(system_prompt, latest_user);
    if !model.is_empty() {
        base = semantic_key(&base, model);
    }
    if !provider.is_empty() {
        base = semantic_key(&base, provider);
    }
    base
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_keys() {
        let a = key_for_request("sys", "hi", "gpt-4", "openai");
        let b = key_for_request("sys", "hi", "gpt-4", "openai");
        assert_eq!(a, b);
        assert_ne!(a, key_for_request("sys", "hi", "gpt-4", "anthropic"));
    }
}
