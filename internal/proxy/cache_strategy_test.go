package proxy_test

import (
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/proxy"
)

func TestCacheKeyStrategy_latestOnlyHitsDespiteToolHistory(t *testing.T) {
	h := newStrategyHandler(t, cache.StrategyLatestOnly, 4)
	bodyA := agentBody("call_db", "Database migration active.", "Run tests again")
	bodyB := agentBody("call_css", "CSS layout padding updated.", "Run tests again")

	if post(h, bodyA).Header().Get("X-KortoLabs-Cache") != "" {
		t.Fatal("first request should miss")
	}
	if got := post(h, bodyB).Header().Get("X-KortoLabs-Cache"); got != "HIT" {
		t.Fatalf("latest_only should hit when only final user text matches, got %q", got)
	}
}

func TestCacheKeyStrategy_windowNMissesOnDivergentToolHistory(t *testing.T) {
	h := newStrategyHandler(t, cache.StrategyWindowN, 4)
	bodyA := agentBody("call_db", "Database migration active.", "Run tests again")
	bodyB := agentBody("call_css", "CSS layout padding updated.", "Run tests again")

	if post(h, bodyA).Header().Get("X-KortoLabs-Cache") != "" {
		t.Fatal("first request should miss")
	}
	if got := post(h, bodyB).Header().Get("X-KortoLabs-Cache"); got == "HIT" {
		t.Fatal("window_n must not hit across divergent tool outputs with the same final user phrase")
	}
}

func TestCacheKeyStrategy_fullDigestMissesOnAnyHistoryChange(t *testing.T) {
	h := newStrategyHandler(t, cache.StrategyFullDigest, 4)
	bodyA := `{"model":"gpt-4o","stream":true,"messages":[{"role":"system","content":"sys"},{"role":"user","content":"step one"}]}`
	bodyB := `{"model":"gpt-4o","stream":true,"messages":[{"role":"system","content":"sys"},{"role":"user","content":"step two"}]}`

	if post(h, bodyA).Header().Get("X-KortoLabs-Cache") != "" {
		t.Fatal("first request should miss")
	}
	if got := post(h, bodyB).Header().Get("X-KortoLabs-Cache"); got == "HIT" {
		t.Fatal("full_digest must miss when any turn differs")
	}
}

func newStrategyHandler(t *testing.T, strategy cache.CacheKeyStrategy, window int) http.Handler {
	t.Helper()

	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		flusher := w.(http.Flusher)
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		id := r.Header.Get("X-Test-Id")
		if id == "" {
			id = "t"
		}
		chunk := `{"id":"` + id + `","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"ok"}}]}`
		_, _ = w.Write([]byte("data: " + chunk + "\n\n"))
		flusher.Flush()
		_, _ = w.Write([]byte("data: [DONE]\n\n"))
		flusher.Flush()
	}))
	t.Cleanup(upstream.Close)

	store, err := cache.Open(filepath.Join(t.TempDir(), "cache.db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })

	handler, err := proxy.NewHandler(proxy.Options{
		UpstreamURL:       upstream.URL,
		EnableCache:       true,
		EnableRedaction:   false,
		EnableCompression: false,
		CacheHitDelay:     0,
		CacheKeyStrategy:  strategy,
		CacheWindowSize:   window,
	}, store, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	return handler
}

func post(h http.Handler, body string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(http.MethodPost, "/v1/chat/completions", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rr := httptest.NewRecorder()
	h.ServeHTTP(rr, req)
	time.Sleep(5 * time.Millisecond)
	return rr
}

func agentBody(toolID, toolContent, latestUser string) string {
	return `{"model":"gpt-4o","stream":true,"messages":[` +
		`{"role":"system","content":"sys"},` +
		`{"role":"user","content":"start"},` +
		`{"role":"tool","content":"` + toolContent + `","tool_call_id":"` + toolID + `"},` +
		`{"role":"user","content":"` + latestUser + `"}` +
		`]}`
}
