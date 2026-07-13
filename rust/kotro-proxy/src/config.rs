//! Runtime configuration — mirrors `internal/config/config.go`.

use std::env;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_url: String,
    pub fallback_url: Option<String>,
    pub fallback_model: Option<String>,
    pub cache_db_path: String,
    pub cache_ttl: Duration,
    pub eviction_interval: Duration,
    pub cache_hit_delay: Duration,
    pub enable_cache: bool,
    pub enable_redaction: bool,
    pub enable_compression: bool,
    pub enable_shrink: bool,
    pub enable_vector_cache: bool,
    pub enable_pprof: bool,
    pub trust_upstream_gateway: bool,
    pub trusted_proxy_cidrs: String,
    pub compressor_max_scopes: u64,
    pub compressor_scope_ttl: Duration,
    pub cache_key_strategy: crate::cache::CacheKeyStrategy,
    pub cache_window_size: usize,
    pub metrics_addr: String,
    pub enable_metrics: bool,
    pub local_model_pattern: Option<String>,
    pub local_upstream_url: Option<String>,
    pub moe_default_model: String,
    /// Model name for the `Micro` complexity tier (cheap API model).
    /// Example: `claude-haiku-4-5-20251001` or `gpt-4o-mini`.
    /// When unset, Micro-tier requests use the default configured model.
    pub cheap_model: Option<String>,
    /// Optional upstream base URL for the cheap model (e.g. a different provider).
    /// When unset, cheap-model requests are forwarded to `upstream_url`.
    pub cheap_model_url: Option<String>,
    /// Number of identical `(tool_name, args)` calls in one conversation before
    /// the agent loop circuit breaker fires. Default: 3. Set to 0 to disable.
    pub tool_loop_threshold: u32,
    /// Scan tool-call results and user messages for prompt injection patterns.
    /// Default: `true`. Disable with `KOTRO_ENABLE_INJECTION_SCAN=false`.
    pub enable_injection_scan: bool,
    /// Block requests that trigger the injection scanner (HTTP 400) instead of
    /// only warning. Default: `false`. Enable with `KOTRO_INJECTION_BLOCK=true`.
    pub injection_block_on_detection: bool,
    /// Estimated-token limit per scope per session (0 = unlimited).
    /// Set with `KOTRO_SESSION_TOKEN_BUDGET=<n>`.
    pub session_token_budget: u64,
    /// Block requests that exceed the session budget (HTTP 429) instead of only
    /// setting `X-Kotro-Budget-Remaining: 0`. Default: `false`.
    /// Enable with `KOTRO_BUDGET_BLOCK=true`.
    pub budget_block_on_exceeded: bool,
    /// Maximum thinking/reasoning tokens allowed per request for known reasoning
    /// models (Claude Opus 4.x, o1/o3 family). `0` = no cap (default).
    /// Set with `KOTRO_MAX_THINKING_TOKENS=<n>`.
    pub max_thinking_tokens: u64,
    /// Block requests to reasoning models entirely (HTTP 403) instead of capping
    /// their thinking budget. Default: `false`. Enable with `KOTRO_REASONING_BLOCK=true`.
    pub reasoning_block: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: ":8080".into(),
            upstream_url: "http://127.0.0.1:9000".into(),
            fallback_url: None,
            fallback_model: None,
            cache_db_path: "./kotro-cache.db".into(),
            cache_ttl: Duration::from_secs(24 * 3600),
            eviction_interval: Duration::from_secs(10 * 60),
            cache_hit_delay: Duration::from_millis(2),
            enable_cache: true,
            enable_redaction: true,
            enable_compression: true,
            enable_shrink: true,
            enable_vector_cache: true,
            enable_pprof: false,
            trust_upstream_gateway: false,
            trusted_proxy_cidrs: String::new(),
            compressor_max_scopes: 10_000,
            compressor_scope_ttl: Duration::from_secs(3600),
            cache_key_strategy: crate::cache::CacheKeyStrategy::WindowN,
            cache_window_size: 4,
            metrics_addr: "127.0.0.1:9090".into(),
            enable_metrics: true,
            local_model_pattern: None,
            local_upstream_url: None,
            moe_default_model: "llama3".into(),
            cheap_model: None,
            cheap_model_url: None,
            tool_loop_threshold: 3,
            enable_injection_scan: true,
            injection_block_on_detection: false,
            session_token_budget: 0,
            budget_block_on_exceeded: false,
            max_thinking_tokens: 0,
            reasoning_block: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let defaults = Self::default();

        let profile = env_or("KOTRO_PROFILE", String::new());
        let mut strategy = crate::cache::parse_cache_key_strategy(
            &env_or("KOTRO_CACHE_KEY_STRATEGY", String::new()),
        );
        let enable_redaction = env_bool("KOTRO_ENABLE_REDACTION", defaults.enable_redaction);
        let mut enable_compression = env_bool("KOTRO_ENABLE_COMPRESSION", defaults.enable_compression);

        match profile.as_str() {
            "cursor" => {
                strategy = crate::cache::CacheKeyStrategy::WindowN;
                enable_compression = true;
            }
            "copilot" => strategy = crate::cache::CacheKeyStrategy::FullDigest,
            "continue" => strategy = crate::cache::CacheKeyStrategy::WindowN,
            _ => {}
        }

        if !enable_redaction {
            tracing::warn!(
                profile = %profile,
                "PII redaction is disabled; secrets may be forwarded upstream"
            );
        }

        let mut fallback_url = env_opt("KOTRO_FALLBACK_URL");
        if fallback_url.is_some() {
            if let Some(ref raw) = fallback_url {
                if reqwest::Url::parse(raw).is_err() {
                    tracing::warn!(
                        value = %raw,
                        "invalid KOTRO_FALLBACK_URL; failover disabled"
                    );
                    fallback_url = None;
                }
            }
        }

        Self {
            listen_addr: env_or("KOTRO_LISTEN_ADDR", defaults.listen_addr),
            upstream_url: env_or("KOTRO_UPSTREAM_URL", defaults.upstream_url),
            fallback_url,
            fallback_model: env_opt("KOTRO_FALLBACK_MODEL"),
            cache_db_path: env_or("KOTRO_CACHE_DB", defaults.cache_db_path),
            cache_ttl: env_flexible_duration("KOTRO_CACHE_TTL", defaults.cache_ttl),
            eviction_interval: env_flexible_duration(
                "KOTRO_EVICTION_INTERVAL",
                defaults.eviction_interval,
            ),
            cache_hit_delay: env_flexible_duration(
                "KOTRO_CACHE_HIT_DELAY_MS",
                defaults.cache_hit_delay,
            ),
            enable_cache: env_bool("KOTRO_ENABLE_CACHE", defaults.enable_cache),
            enable_redaction,
            enable_compression,
            enable_shrink: env_bool("KOTRO_ENABLE_SHRINK", defaults.enable_shrink),
            enable_vector_cache: env_bool("KOTRO_ENABLE_VECTOR_CACHE", defaults.enable_vector_cache),
            enable_pprof: env_bool("KOTRO_ENABLE_PPROF", defaults.enable_pprof),
            trust_upstream_gateway: env_bool(
                "KOTRO_TRUST_UPSTREAM_GATEWAY",
                defaults.trust_upstream_gateway,
            ),
            trusted_proxy_cidrs: env_or("KOTRO_TRUSTED_PROXY_CIDRS", defaults.trusted_proxy_cidrs),
            compressor_max_scopes: env_u64("KOTRO_COMPRESSOR_MAX_SCOPES", defaults.compressor_max_scopes),
            compressor_scope_ttl: env_flexible_duration(
                "KOTRO_COMPRESSOR_SCOPE_TTL",
                defaults.compressor_scope_ttl,
            ),
            cache_key_strategy: strategy,
            cache_window_size: env_usize("KOTRO_CACHE_WINDOW_SIZE", defaults.cache_window_size),
            metrics_addr: env_or("KOTRO_METRICS_ADDR", defaults.metrics_addr),
            enable_metrics: env_bool("KOTRO_ENABLE_METRICS", defaults.enable_metrics),
            local_model_pattern: env_opt("KOTRO_LOCAL_MODEL_PATTERN"),
            local_upstream_url: env_opt("KOTRO_LOCAL_UPSTREAM_URL"),
            moe_default_model: env_or("KOTRO_MOE_DEFAULT_MODEL", defaults.moe_default_model),
            cheap_model: env_opt("KOTRO_CHEAP_MODEL"),
            cheap_model_url: env_opt("KOTRO_CHEAP_MODEL_URL"),
            tool_loop_threshold: env_u64("KOTRO_TOOL_LOOP_THRESHOLD", defaults.tool_loop_threshold as u64) as u32,
            enable_injection_scan: env_bool("KOTRO_ENABLE_INJECTION_SCAN", defaults.enable_injection_scan),
            injection_block_on_detection: env_bool("KOTRO_INJECTION_BLOCK", defaults.injection_block_on_detection),
            session_token_budget: env_u64("KOTRO_SESSION_TOKEN_BUDGET", defaults.session_token_budget),
            budget_block_on_exceeded: env_bool("KOTRO_BUDGET_BLOCK", defaults.budget_block_on_exceeded),
            max_thinking_tokens: env_u64("KOTRO_MAX_THINKING_TOKENS", defaults.max_thinking_tokens),
            reasoning_block: env_bool("KOTRO_REASONING_BLOCK", defaults.reasoning_block),
        }
    }
}

fn env_or(key: &str, fallback: String) -> String {
    env::var(key).unwrap_or(fallback)
}

fn env_opt(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.is_empty())
}

fn env_bool(key: &str, fallback: bool) -> bool {
    match env::var(key) {
        Ok(v) => v.parse().unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    match env::var(key) {
        Ok(v) => v.parse().unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn env_usize(key: &str, fallback: usize) -> usize {
    match env::var(key) {
        Ok(v) => v.parse().ok().filter(|n| *n > 0).unwrap_or(fallback),
        Err(_) => fallback,
    }
}

fn env_flexible_duration(key: &str, fallback: Duration) -> Duration {
    match env::var(key) {
        Ok(v) if !v.is_empty() => {
            if key.ends_with("_MS") {
                if let Ok(ms) = v.parse::<u64>() {
                    return Duration::from_millis(ms);
                }
            }
            parse_go_duration(&v).unwrap_or(fallback)
        }
        _ => fallback,
    }
}

fn unit_duration(value: &str, unit: &str) -> Option<Duration> {
    let num = value.strip_suffix(unit)?.parse::<u64>().ok()?;
    Some(match unit {
        "ns" => Duration::from_nanos(num),
        "us" | "µs" => Duration::from_micros(num),
        "ms" => Duration::from_millis(num),
        "s" => Duration::from_secs(num),
        "m" => Duration::from_secs(num * 60),
        "h" => Duration::from_secs(num * 3600),
        _ => return None,
    })
}

fn parse_go_duration(v: &str) -> Option<Duration> {
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    let mut total = Duration::ZERO;
    let mut i = 0;
    let bytes = v.as_bytes();

    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        let digits = &v[start..i];

        if let Some(unit) = ["ms", "µs", "us", "ns"]
            .into_iter()
            .find(|unit| v[i..].starts_with(unit))
        {
            total += unit_duration(&format!("{digits}{unit}"), unit)?;
            i += unit.len();
            continue;
        }

        if i >= bytes.len() {
            return None;
        }

        let unit = match bytes[i] as char {
            'h' => "h",
            'm' => "m",
            's' => "s",
            _ => return None,
        };
        total += unit_duration(&format!("{digits}{unit}"), unit)?;
        i += 1;
    }

    if total > Duration::ZERO {
        Some(total)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duration_suffixes() {
        assert_eq!(parse_go_duration("24h"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_go_duration("10m"), Some(Duration::from_secs(600)));
        assert_eq!(parse_go_duration("2ms"), Some(Duration::from_millis(2)));
        assert_eq!(parse_go_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_go_duration("250us"), Some(Duration::from_micros(250)));
        assert_eq!(parse_go_duration("100ns"), Some(Duration::from_nanos(100)));
    }
}
