package cache_test

import (
	"encoding/json"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/models"
)

func TestCacheKeyStrategy_CollisionPrevention(t *testing.T) {
	reqDBChange := &models.ChatCompletionRequest{
		Model: "gpt-4o",
		Messages: []models.ChatMessage{
			{Role: "system", Content: mustFlex(`"System configuration prompt"`)},
			{Role: "user", Content: mustFlex(`"Execute runtime tests"`)},
			{Role: "tool", Content: mustFlex(`"Database migration active: altered target index metrics."`), ToolCallID: "call_db"},
			{Role: "user", Content: mustFlex(`"Run tests again"`)},
		},
	}

	reqCSSChange := &models.ChatCompletionRequest{
		Model: "gpt-4o",
		Messages: []models.ChatMessage{
			{Role: "system", Content: mustFlex(`"System configuration prompt"`)},
			{Role: "user", Content: mustFlex(`"Execute runtime tests"`)},
			{Role: "tool", Content: mustFlex(`"CSS style change active: updated layout padding values."`), ToolCallID: "call_css"},
			{Role: "user", Content: mustFlex(`"Run tests again"`)},
		},
	}

	matL1 := reqDBChange.ExtractCacheKeyMaterial(cache.StrategyLatestOnly, 4)
	matL2 := reqCSSChange.ExtractCacheKeyMaterial(cache.StrategyLatestOnly, 4)
	if cache.KeyForRequestWithStrategy("scope-x", "gpt-4o", "openai", matL1) !=
		cache.KeyForRequestWithStrategy("scope-x", "gpt-4o", "openai", matL2) {
		t.Fatal("latest_only must intentionally collide when only the final user phrase matches")
	}

	matW1 := reqDBChange.ExtractCacheKeyMaterial(cache.StrategyWindowN, 4)
	matW2 := reqCSSChange.ExtractCacheKeyMaterial(cache.StrategyWindowN, 4)
	keyW1 := cache.KeyForRequestWithStrategy("scope-x", "gpt-4o", "openai", matW1)
	keyW2 := cache.KeyForRequestWithStrategy("scope-x", "gpt-4o", "openai", matW2)
	if keyW1 == keyW2 {
		t.Fatal("window_n must not collide across divergent tool outputs with the same final user phrase")
	}
}

func TestKeyForRequestWithStrategyProviderIsolation(t *testing.T) {
	material := []byte("sys||hi")
	openai := cache.KeyForRequestWithStrategy("default:default", "gpt-4", "openai", material)
	anthropic := cache.KeyForRequestWithStrategy("default:default", "gpt-4", "anthropic", material)
	if openai == anthropic {
		t.Fatal("openai and anthropic keys must not collide")
	}
}

func TestKeyForRequestWithStrategyTenantIsolation(t *testing.T) {
	material := []byte("sys||hi")
	tenantA := cache.KeyForRequestWithStrategy("tenant-a:session-1", "gpt-4", "openai", material)
	tenantB := cache.KeyForRequestWithStrategy("tenant-b:session-1", "gpt-4", "openai", material)
	if tenantA == tenantB {
		t.Fatal("different tenant scopes must not share cache keys")
	}
}

func mustFlex(raw string) models.FlexContent {
	var f models.FlexContent
	_ = json.Unmarshal([]byte(raw), &f)
	return f
}
