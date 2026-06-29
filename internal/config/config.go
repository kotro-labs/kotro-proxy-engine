// Package config loads runtime settings for the KortoLabs proxy engine.
package config

import (
	"os"
	"strconv"
	"time"
)

// Config holds all tunable proxy parameters.
type Config struct {
	ListenAddr   string
	UpstreamURL  string
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
}

// Load reads configuration from environment variables with sensible defaults
// for local development against the mock upstream on port 9000.
func Load() Config {
	return Config{
		ListenAddr:        envOr("KORTO_LISTEN_ADDR", ":8080"),
		UpstreamURL:       envOr("KORTO_UPSTREAM_URL", "http://127.0.0.1:9000"),
		CacheDBPath:         envOr("KORTO_CACHE_DB", "./kortolabs-cache.db"),
		ReadTimeout:         envDurationOr("KORTO_READ_TIMEOUT", 30*time.Second),
		WriteTimeout:        envDurationOr("KORTO_WRITE_TIMEOUT", 0),
		IdleTimeout:         envDurationOr("KORTO_IDLE_TIMEOUT", 120*time.Second),
		EnableCache:         envBoolOr("KORTO_ENABLE_CACHE", true),
		EnableRedaction:     envBoolOr("KORTO_ENABLE_REDACTION", true),
		EnableCompression:   envBoolOr("KORTO_ENABLE_COMPRESSION", true),
		CacheHitDelay:       envDurationOr("KORTO_CACHE_HIT_DELAY_MS", 2*time.Millisecond),
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
