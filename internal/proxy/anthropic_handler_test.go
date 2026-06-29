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

func TestAnthropicCacheMissThenHit(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		flusher := w.(http.Flusher)
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)

		writeAnthropicSSE(w, flusher, "content_block_delta", `{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi "}}`)
		time.Sleep(5 * time.Millisecond)
		writeAnthropicSSE(w, flusher, "message_stop", `{"type":"message_stop"}`)
	}))
	defer upstream.Close()

	dir := t.TempDir()
	store, err := cache.Open(filepath.Join(dir, "cache.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	handler, err := proxy.NewAnthropicHandlerFromURL(upstream.URL, store, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}

	body := `{"model":"claude-3-5-sonnet-20241022","max_tokens":64,"stream":true,"system":"sys","messages":[{"role":"user","content":"ping"}]}`
	req := httptest.NewRequest(http.MethodPost, "/v1/messages", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")

	w1 := httptest.NewRecorder()
	handler.ServeHTTP(w1, req)
	if w1.Code != http.StatusOK {
		t.Fatalf("miss status %d body %s", w1.Code, w1.Body.String())
	}

	req2 := httptest.NewRequest(http.MethodPost, "/v1/messages", strings.NewReader(body))
	req2.Header.Set("Content-Type", "application/json")
	w2 := httptest.NewRecorder()
	handler.ServeHTTP(w2, req2)

	if w2.Header().Get("X-KortoLabs-Cache") != "HIT" {
		t.Fatal("expected anthropic cache hit header")
	}
}

func TestAnthropicForwardsAPIKey(t *testing.T) {
	var apiKey string
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		apiKey = r.Header.Get("x-api-key")
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		writeAnthropicSSE(w, w.(http.Flusher), "message_stop", `{"type":"message_stop"}`)
	}))
	defer upstream.Close()

	dir := t.TempDir()
	store, _ := cache.Open(filepath.Join(dir, "c.db"))
	defer store.Close()
	handler, _ := proxy.NewAnthropicHandlerFromURL(upstream.URL, store, slog.New(slog.NewTextHandler(io.Discard, nil)))

	body := `{"model":"claude-3-5-sonnet-20241022","max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hi"}]}`
	req := httptest.NewRequest(http.MethodPost, "/v1/messages", strings.NewReader(body))
	req.Header.Set("x-api-key", "sk-ant-test-key")
	handler.ServeHTTP(httptest.NewRecorder(), req)

	if apiKey != "sk-ant-test-key" {
		t.Fatalf("x-api-key not forwarded: %q", apiKey)
	}
}

func writeAnthropicSSE(w http.ResponseWriter, flusher http.Flusher, event, data string) {
	_, _ = w.Write([]byte("event: " + event + "\ndata: " + data + "\n\n"))
	flusher.Flush()
}
