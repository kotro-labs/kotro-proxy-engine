package cache_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/cache"
)

func TestSemanticKeyDeterministic(t *testing.T) {
	a := cache.SemanticKey("system", "hello")
	b := cache.SemanticKey("system", "hello")
	if a != b {
		t.Fatal("semantic key not deterministic")
	}
	if cache.SemanticKey("system", "world") == a {
		t.Fatal("different prompts should produce different keys")
	}
}

func TestKeyForRequestProviderIsolation(t *testing.T) {
	openai := cache.KeyForRequest("sys", "hi", "gpt-4", "openai", "default:default")
	anthropic := cache.KeyForRequest("sys", "hi", "gpt-4", "anthropic", "default:default")
	if openai == anthropic {
		t.Fatal("openai and anthropic keys must not collide")
	}
}

func TestKeyForRequestTenantIsolation(t *testing.T) {
	tenantA := cache.KeyForRequest("sys", "hi", "gpt-4", "openai", "tenant-a:session-1")
	tenantB := cache.KeyForRequest("sys", "hi", "gpt-4", "openai", "tenant-b:session-1")
	if tenantA == tenantB {
		t.Fatal("different tenant scopes must not share cache keys")
	}
}

func TestStoreRoundTrip(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "test.db")

	store, err := cache.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	entry := cache.Entry{
		Key:    "abc123",
		RawSSE: []byte("data: {\"choices\":[]}\n\ndata: [DONE]\n\n"),
		Model:  "gpt-4",
	}
	if err := store.Put(entry); err != nil {
		t.Fatal(err)
	}

	got, err := store.Get("abc123")
	if err != nil {
		t.Fatal(err)
	}
	if got == nil {
		t.Fatal("cache miss on existing key")
	}
	if string(got.RawSSE) != string(entry.RawSSE) {
		t.Fatalf("sse mismatch: %q vs %q", got.RawSSE, entry.RawSSE)
	}

	miss, err := store.Get("missing")
	if err != nil {
		t.Fatal(err)
	}
	if miss != nil {
		t.Fatal("expected nil on miss")
	}

	_ = store.Close()
	if _, err := os.Stat(path); err != nil {
		t.Fatal("db file should exist on disk")
	}
}
