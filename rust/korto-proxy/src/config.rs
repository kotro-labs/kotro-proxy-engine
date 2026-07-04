//! Runtime configuration — mirrors `internal/config/config.go`.

use std::env;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_url: String,
    pub cache_db_path: String,
    pub cache_ttl: Duration,
    pub eviction_interval: Duration,
    pub cache_hit_delay: Duration,
    pub enable_cache: bool,
    pub enable_redaction: bool,
    pub enable_compression: bool,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: ":8080".into(),
            upstream_url: "http://127.0.0.1:9000".into(),
            cache_db_path: "./kortolabs-cache.db".into(),
            cache_ttl: Duration::from_secs(24 * 3600),
            eviction_interval: Duration::from_secs(10 * 60),
            cache_hit_delay: Duration::from_millis(2),
            enable_cache: true,
            enable_redaction: true,
            enable_compression: true,
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
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let defaults = Self::default();
        Self {
            listen_addr: env_or("KORTO_LISTEN_ADDR", defaults.listen_addr),
            upstream_url: env_or("KORTO_UPSTREAM_URL", defaults.upstream_url),
            cache_db_path: env_or("KORTO_CACHE_DB", defaults.cache_db_path),
            cache_ttl: env_flexible_duration("KORTO_CACHE_TTL", defaults.cache_ttl),
            eviction_interval: env_flexible_duration(
                "KORTO_EVICTION_INTERVAL",
                defaults.eviction_interval,
            ),
            cache_hit_delay: env_flexible_duration(
                "KORTO_CACHE_HIT_DELAY_MS",
                defaults.cache_hit_delay,
            ),
            enable_cache: env_bool("KORTO_ENABLE_CACHE", defaults.enable_cache),
            enable_redaction: env_bool("KORTO_ENABLE_REDACTION", defaults.enable_redaction),
            enable_compression: env_bool("KORTO_ENABLE_COMPRESSION", defaults.enable_compression),
            enable_pprof: env_bool("KORTO_ENABLE_PPROF", defaults.enable_pprof),
            trust_upstream_gateway: env_bool(
                "KORTO_TRUST_UPSTREAM_GATEWAY",
                defaults.trust_upstream_gateway,
            ),
            trusted_proxy_cidrs: env_or("KORTO_TRUSTED_PROXY_CIDRS", defaults.trusted_proxy_cidrs),
            compressor_max_scopes: env_u64("KORTO_COMPRESSOR_MAX_SCOPES", defaults.compressor_max_scopes),
            compressor_scope_ttl: env_flexible_duration(
                "KORTO_COMPRESSOR_SCOPE_TTL",
                defaults.compressor_scope_ttl,
            ),
            cache_key_strategy: crate::cache::parse_cache_key_strategy(
                &env_or("KORTO_CACHE_KEY_STRATEGY", String::new()),
            ),
            cache_window_size: env_usize("KORTO_CACHE_WINDOW_SIZE", defaults.cache_window_size),
            metrics_addr: env_or("KORTO_METRICS_ADDR", defaults.metrics_addr),
            enable_metrics: env_bool("KORTO_ENABLE_METRICS", defaults.enable_metrics),
            local_model_pattern: env_opt("KORTO_LOCAL_MODEL_PATTERN"),
            local_upstream_url: env_opt("KORTO_LOCAL_UPSTREAM_URL"),
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
