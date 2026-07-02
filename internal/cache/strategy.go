package cache

import (
	"fmt"
	"strings"
)

// CacheKeyStrategy selects how conversation context is folded into cache keys.
type CacheKeyStrategy string

const (
	StrategyLatestOnly CacheKeyStrategy = "latest_only" // Legacy: system + last user string
	StrategyWindowN    CacheKeyStrategy = "window_n"    // Default: system + last N execution steps
	StrategyFullDigest CacheKeyStrategy = "full_digest" // Strict: canonical JSON of full history
)

// ParseStrategy maps configuration strings to a strategy, defaulting to window_n.
func ParseStrategy(s string) (CacheKeyStrategy, error) {
	switch CacheKeyStrategy(strings.ToLower(strings.TrimSpace(s))) {
	case StrategyLatestOnly:
		return StrategyLatestOnly, nil
	case StrategyWindowN, "":
		return StrategyWindowN, nil
	case StrategyFullDigest:
		return StrategyFullDigest, nil
	default:
		return StrategyWindowN, fmt.Errorf("unknown cache key strategy %q; falling back to window_n", s)
	}
}
