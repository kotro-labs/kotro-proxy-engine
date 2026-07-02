//! Semantic cache keying — mirrors `internal/cache/semantic.go` + `strategy.go`.

use sha2::{Digest, Sha256};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKeyStrategy {
    LatestOnly,
    WindowN,
    FullDigest,
}

impl FromStr for CacheKeyStrategy {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(parse_cache_key_strategy(s))
    }
}

/// Parses `KORTO_CACHE_KEY_STRATEGY`, falling back to `window_n` and logging on unknown values.
pub fn parse_cache_key_strategy(raw: &str) -> CacheKeyStrategy {
    match raw.to_lowercase().trim() {
        "latest_only" => CacheKeyStrategy::LatestOnly,
        "full_digest" => CacheKeyStrategy::FullDigest,
        "window_n" | "" => CacheKeyStrategy::WindowN,
        other => {
            tracing::warn!(
                value = other,
                "unknown KORTO_CACHE_KEY_STRATEGY; falling back to window_n"
            );
            CacheKeyStrategy::WindowN
        }
    }
}

fn semantic_key(system_prompt: &str, latest_user: &str) -> String {
    let mut h = Sha256::new();
    h.update(system_prompt.as_bytes());
    h.update([0u8]);
    h.update(latest_user.as_bytes());
    hex_encode(h.finalize().as_slice())
}

/// Hashes prompt state, model, provider, and isolation scope for lookup (legacy latest_only chain).
pub fn key_for_request(
    system_prompt: &str,
    latest_user: &str,
    model: &str,
    provider: &str,
    isolation_scope: &str,
) -> String {
    let mut base = semantic_key(system_prompt, latest_user);
    if !model.is_empty() {
        base = semantic_key(&base, model);
    }
    if !provider.is_empty() {
        base = semantic_key(&base, provider);
    }
    if !isolation_scope.is_empty() {
        base = semantic_key(&base, isolation_scope);
    }
    base
}

/// Computes a cache key from strategy-derived material.
pub fn generate_cache_key(scope_key: &str, model: &str, provider: &str, material: &[u8]) -> String {
    let mut hasher = Sha256::new();
    if !provider.is_empty() {
        hasher.update(provider.as_bytes());
        hasher.update([0u8]);
    }
    hasher.update(material);
    let digest = hex_encode(hasher.finalize().as_slice());
    format!("cache:{scope_key}:{model}:{digest}")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_keys() {
        let a = key_for_request("sys", "hi", "gpt-4", "openai", "default:default");
        let b = key_for_request("sys", "hi", "gpt-4", "openai", "default:default");
        assert_eq!(a, b);
        assert_ne!(a, key_for_request("sys", "hi", "gpt-4", "anthropic", "default:default"));
    }

    #[test]
    fn tenant_scopes_do_not_collide() {
        let a = key_for_request("sys", "hi", "gpt-4", "openai", "tenant-a:session-1");
        let b = key_for_request("sys", "hi", "gpt-4", "openai", "tenant-b:session-1");
        assert_ne!(a, b);
    }

    #[test]
    fn generate_cache_key_splits_providers() {
        let material = b"sys||hi";
        let openai = generate_cache_key("default:default", "gpt-4", "openai", material);
        let anthropic = generate_cache_key("default:default", "gpt-4", "anthropic", material);
        assert_ne!(openai, anthropic);
    }

    #[test]
    fn invalid_strategy_falls_back_to_window_n() {
        assert_eq!(
            super::parse_cache_key_strategy("not-a-strategy"),
            CacheKeyStrategy::WindowN
        );
    }
}
