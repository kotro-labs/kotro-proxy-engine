// Package config loads runtime settings for the KortoLabs proxy engine.
package config

import (
	"log/slog"
	"net/url"
	"os"
	"strconv"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
)

// Config holds all tunable proxy parameters.
type Config struct {
	ListenAddr   string
	UpstreamURL  string
	FallbackURL  string
	CacheDBPath  string
	ReadTimeout  time.Duration
	WriteTimeout time.Duration
	IdleTimeout  time.Duration

	// Feature toggles — all enabled by default.
	EnableCache       bool
	EnableRedaction   bool
	EnableCompression bool

	// CacheHitDelay simulates streaming cadence on cache hits (0 = minimal flush).
	CacheHitDelay time.Duration

	// EnablePprof exposes /debug/pprof for local profiling and leak audits.
	EnablePprof bool

	// CacheTTL is entry lifetime (0 disables expiration). Accepts Go duration strings.
	CacheTTL time.Duration

	// CacheEvictionInterval is the background sweep cadence for expired keys.
	CacheEvictionInterval time.Duration

	// MaxRequestBodyBytes caps JSON request body size (default 10 MiB).
	MaxRequestBodyBytes int64

	// TrustUpstreamGateway honors X-Tenant-ID from trusted proxy CIDRs only.
	TrustUpstreamGateway bool

	// TrustedProxyCIDRs is a comma-separated list of CIDRs allowed to set scope headers.
	TrustedProxyCIDRs string

	// CompressorMaxScopes bounds in-memory compressor scope entries.
	CompressorMaxScopes int

	// CompressorScopeTTL evicts idle compressor scopes after this duration.
	CompressorScopeTTL time.Duration

	// EnableMetrics exposes GET /metrics for Prometheus scraping.
	EnableMetrics bool

	// MetricsAddr is the loopback-isolated listener for /metrics and /dashboard.
	MetricsAddr string

	// CacheKeyStrategy selects how multi-turn context is hashed into cache keys.
	CacheKeyStrategy cache.CacheKeyStrategy

	// CacheWindowSize is the number of trailing non-system turns for window_n keying.
	CacheWindowSize int
}

// Load reads configuration from environment variables with sensible defaults
// for local development against the mock upstream on port 9000.
func Load() Config {
	strategy, err := cache.ParseStrategy(envOr("KORTO_CACHE_KEY_STRATEGY", ""))
	if err != nil {
		slog.Default().Warn(
			"invalid KORTO_CACHE_KEY_STRATEGY; falling back to window_n",
			"err", err,
			"value", os.Getenv("KORTO_CACHE_KEY_STRATEGY"),
		)
	}

	profile := envOr("KORTO_PROFILE", "")

	enableRedaction := envBoolOr("KORTO_ENABLE_REDACTION", true)
	enableCompression := envBoolOr("KORTO_ENABLE_COMPRESSION", true)

	// Apply IDE Profile Presets
	switch profile {
	case "cursor":
		strategy = cache.StrategyWindowN
		enableCompression = true
	case "copilot":
		strategy = cache.StrategyFullDigest
	case "continue":
		strategy = cache.StrategyWindowN
	}

	if !enableRedaction {
		slog.Default().Warn(
			"PII redaction is disabled; secrets may be forwarded upstream",
			"profile", profile,
			"KORTO_ENABLE_REDACTION", os.Getenv("KORTO_ENABLE_REDACTION"),
		)
	}

	fallbackURL := envOr("KORTO_FALLBACK_URL", "")
	if os.Getenv("KORTO_FALLBACK_URL") != "" {
		if _, err := url.Parse(fallbackURL); err != nil {
			slog.Default().Warn(
				"invalid KORTO_FALLBACK_URL; failover disabled",
				"err", err,
				"value", fallbackURL,
			)
			fallbackURL = ""
		}
	}

	return Config{
		ListenAddr:            envOr("KORTO_LISTEN_ADDR", ":8080"),
		UpstreamURL:           envOr("KORTO_UPSTREAM_URL", "http://127.0.0.1:9000"),
		FallbackURL:           fallbackURL,
		CacheDBPath:           envOr("KORTO_CACHE_DB", "./kortolabs-cache.db"),
		ReadTimeout:           envDurationOr("KORTO_READ_TIMEOUT", 30*time.Second),
		WriteTimeout:          envDurationOr("KORTO_WRITE_TIMEOUT", 0),
		IdleTimeout:           envDurationOr("KORTO_IDLE_TIMEOUT", 120*time.Second),
		EnableCache:           envBoolOr("KORTO_ENABLE_CACHE", true),
		EnableRedaction:       enableRedaction,
		EnableCompression:     enableCompression,
		CacheHitDelay:         envDurationOr("KORTO_CACHE_HIT_DELAY_MS", 2*time.Millisecond),
		EnablePprof:           envBoolOr("KORTO_ENABLE_PPROF", false),
		CacheTTL:              envFlexibleDurationOr("KORTO_CACHE_TTL", 24*time.Hour),
		CacheEvictionInterval: envFlexibleDurationOr("KORTO_EVICTION_INTERVAL", 10*time.Minute),
		MaxRequestBodyBytes:   envInt64Or("KORTO_MAX_REQUEST_BODY_BYTES", 10<<20),
		TrustUpstreamGateway:  envBoolOr("KORTO_TRUST_UPSTREAM_GATEWAY", false),
		TrustedProxyCIDRs:     envOr("KORTO_TRUSTED_PROXY_CIDRS", ""),
		CompressorMaxScopes:   int(envInt64Or("KORTO_COMPRESSOR_MAX_SCOPES", 10_000)),
		CompressorScopeTTL:    envFlexibleDurationOr("KORTO_COMPRESSOR_SCOPE_TTL", time.Hour),
		EnableMetrics:         envBoolOr("KORTO_ENABLE_METRICS", true),
		MetricsAddr:           envOr("KORTO_METRICS_ADDR", "127.0.0.1:9090"),
		CacheKeyStrategy:      strategy,
		CacheWindowSize:       int(envInt64Or("KORTO_CACHE_WINDOW_SIZE", 4)),
	}
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func envDurationOr(key string, fallback time.Duration) time.Duration {
	if v := os.Getenv(key); v != "" {
		if ms, err := strconv.Atoi(v); err == nil {
			// Accept raw seconds for timeout keys, milliseconds for *_MS keys.
			if len(key) >= 3 && key[len(key)-3:] == "_MS" {
				return time.Duration(ms) * time.Millisecond
			}
			return time.Duration(ms) * time.Second
		}
	}
	return fallback
}

func envBoolOr(key string, fallback bool) bool {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	b, err := strconv.ParseBool(v)
	if err != nil {
		return fallback
	}
	return b
}

func envFlexibleDurationOr(key string, fallback time.Duration) time.Duration {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	if d, err := time.ParseDuration(v); err == nil {
		return d
	}
	return envDurationOr(key, fallback)
}

func envInt64Or(key string, fallback int64) int64 {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	n, err := strconv.ParseInt(v, 10, 64)
	if err != nil || n <= 0 {
		return fallback
	}
	return n
}
