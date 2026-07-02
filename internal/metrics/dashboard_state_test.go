package metrics_test

import (
	"testing"
	"time"

	"github.com/kortolabs/proxy-engine/internal/metrics"
)

func TestSnapshotCacheHitRate5m(t *testing.T) {
	reg := metrics.NewRegistry()
	t.Cleanup(reg.Unregister)

	reg.RecordRequest("openai", "/v1/chat/completions", "true", "hit", time.Millisecond)
	reg.RecordRequest("openai", "/v1/chat/completions", "true", "hit", time.Millisecond)
	reg.RecordRequest("openai", "/v1/chat/completions", "true", "miss", time.Millisecond)

	snap := reg.Snapshot()
	if snap.CacheHits5m != 2 || snap.CacheMisses5m != 1 {
		t.Fatalf("unexpected 5m window hits=%d misses=%d", snap.CacheHits5m, snap.CacheMisses5m)
	}
	if snap.CacheHitRate5m < 0.66 || snap.CacheHitRate5m > 0.67 {
		t.Fatalf("expected ~66%% hit rate, got %v", snap.CacheHitRate5m)
	}
	if len(snap.RecentRequests) != 3 {
		t.Fatalf("expected 3 recent requests, got %d", len(snap.RecentRequests))
	}
	if snap.RecentRequests[0].CacheStatus != "miss" {
		t.Fatalf("expected newest first, got %+v", snap.RecentRequests[0])
	}
}

func TestSnapshotCompressorTotals(t *testing.T) {
	reg := metrics.NewRegistry()
	t.Cleanup(reg.Unregister)

	reg.RecordCompression(3, 1024)
	snap := reg.Snapshot()
	if snap.CompressorBytesSaved < 1024 {
		t.Fatalf("expected bytes saved >= 1024, got %v", snap.CompressorBytesSaved)
	}
}
