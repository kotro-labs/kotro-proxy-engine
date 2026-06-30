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
        }
    }
}

fn env_or(key: &str, fallback: String) -> String {
    env::var(key).unwrap_or(fallback)
}

fn env_bool(key: &str, fallback: bool) -> bool {
    match env::var(key) {
        Ok(v) => v.parse().unwrap_or(fallback),
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

fn parse_go_duration(v: &str) -> Option<Duration> {
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let mut total = Duration::ZERO;
    let mut num = String::new();
    for ch in v.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        if num.is_empty() {
            continue;
        }
        let n: u64 = num.parse().ok()?;
        num.clear();
        total += match ch {
            'h' => Duration::from_secs(n * 3600),
            'm' => Duration::from_secs(n * 60),
            's' => Duration::from_secs(n),
            'u' | 'µ' => Duration::from_micros(n),
            'n' => Duration::from_nanos(n),
            _ => return None,
        };
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
    }
}
