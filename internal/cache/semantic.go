// Package cache implements the streaming semantic cache (Feature A).
package cache

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
)

// SemanticKey computes a deterministic hash from the system prompt and the
// latest user message — the two fields that define prompt state for cache lookup.
func SemanticKey(systemPrompt, latestUser string) string {
	h := sha256.New()
	h.Write([]byte(systemPrompt))
	h.Write([]byte{0}) // delimiter prevents collision across field boundaries
	h.Write([]byte(latestUser))
	return hex.EncodeToString(h.Sum(nil))
}

// Entry holds a complete concatenated SSE stream captured on cache miss.
type Entry struct {
	Key        string
	RawSSE     []byte
	Model      string
	CreatedAt  int64
}

// KeyForRequest hashes prompt state, model, provider, and isolation scope for lookup.
func KeyForRequest(systemPrompt, latestUser, model, provider, isolationScope string) string {
	base := SemanticKey(systemPrompt, latestUser)
	if model != "" {
		base = SemanticKey(base, model)
	}
	if provider != "" {
		base = SemanticKey(base, provider)
	}
	if isolationScope != "" {
		base = SemanticKey(base, isolationScope)
	}
	return base
}

// EntryID returns a human-readable cache entry identifier.
func EntryID(key string) string {
	if len(key) > 12 {
		return fmt.Sprintf("cache:%s…", key[:12])
	}
	return "cache:" + key
}
