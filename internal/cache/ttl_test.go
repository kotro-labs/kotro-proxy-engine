package cache_test

import (
	"encoding/json"
	"path/filepath"
	"testing"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
)

func TestStoreTTLExpiresOnRead(t *testing.T) {
	dir := t.TempDir()
	store, err := cache.OpenWithOptions(filepath.Join(dir, "ttl.db"), cache.StoreOptions{
		TTL: time.Hour,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	entry := cache.Entry{
		Key:    "expire-me",
		RawSSE: []byte("data: [DONE]\n\n"),
		Model:  "gpt-4",
	}
	if err := store.Put(entry); err != nil {
		t.Fatal(err)
	}

	got, err := store.Get("expire-me")
	if err != nil || got == nil {
		t.Fatalf("expected immediate hit, got %v err %v", got, err)
	}
}

func TestStoreTTLShortLivedEntry(t *testing.T) {
	dir := t.TempDir()
	// Use a generous TTL: expiresAt is stamped at encode time, before bbolt write.
	// On slow CI runners the write can exceed very short TTLs (15ms), making the
	// immediate Get look like a false miss.
	const ttl = 200 * time.Millisecond
	store, err := cache.OpenWithOptions(filepath.Join(dir, "short.db"), cache.StoreOptions{
		TTL: ttl,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	if err := store.Put(cache.Entry{
		Key:    "short",
		RawSSE: []byte("data: [DONE]\n\n"),
		Model:  "gpt-4",
	}); err != nil {
		t.Fatal(err)
	}

	got, err := store.Get("short")
	if err != nil || got == nil {
		t.Fatalf("expected fresh hit, got %v err %v", got, err)
	}

	time.Sleep(ttl + 100*time.Millisecond)

	got, err = store.Get("short")
	if err != nil {
		t.Fatal(err)
	}
	if got != nil {
		t.Fatal("expected expired entry to behave as cache miss")
	}

	time.Sleep(100 * time.Millisecond)
	got, err = store.Get("short")
	if err != nil {
		t.Fatal(err)
	}
	if got != nil {
		t.Fatal("expected async delete to remove stale key")
	}
}

func TestSweepExpiredKeys(t *testing.T) {
	dir := t.TempDir()
	short, err := cache.OpenWithOptions(filepath.Join(dir, "short.db"), cache.StoreOptions{
		TTL: 1 * time.Millisecond,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer short.Close()

	if err := short.Put(cache.Entry{Key: "gone", RawSSE: []byte("data: y\n\n")}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(5 * time.Millisecond)

	n, err := short.SweepExpired()
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("expected 1 deleted key, got %d", n)
	}

	got, _ := short.Get("gone")
	if got != nil {
		t.Fatal("expected swept key to be gone")
	}
}

func TestLegacyEntryWithoutPrefix(t *testing.T) {
	dir := t.TempDir()
	store, err := cache.Open(filepath.Join(dir, "legacy.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	entry := cache.Entry{
		Key:    "legacy",
		RawSSE: []byte("data: legacy\n\n"),
		Model:  "gpt-4",
	}
	raw, _ := json.Marshal(entry)
	if err := store.PutRaw("legacy", raw); err != nil {
		t.Fatal(err)
	}

	got, err := store.Get("legacy")
	if err != nil {
		t.Fatal(err)
	}
	if got == nil || string(got.RawSSE) != string(entry.RawSSE) {
		t.Fatalf("legacy entry not readable: %+v", got)
	}
}
