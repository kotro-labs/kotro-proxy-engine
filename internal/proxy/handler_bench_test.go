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

func BenchmarkHandlerCacheLookup(b *testing.B) {
	store, err := cache.Open(filepath.Join(b.TempDir(), "bench.db"))
	if err != nil {
		b.Fatal(err)
	}
	defer store.Close()

	key := cache.KeyForRequest("sys", "ping", "gpt-4", "openai", "default:default")
	if err := store.Put(cache.Entry{
		Key:       key,
		RawSSE:    []byte("data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n"),
		Model:     "gpt-4",
		CreatedAt: time.Now().Unix(),
	}); err != nil {
		b.Fatal(err)
	}

	handler, err := proxy.NewHandlerFromURL("http://127.0.0.1:1", store, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		b.Fatal(err)
	}

	body := `{"model":"gpt-4","stream":true,"messages":[{"role":"system","content":"sys"},{"role":"user","content":"ping"}]}`

	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		req := httptest.NewRequest(http.MethodPost, "/v1/chat/completions", strings.NewReader(body))
		handler.ServeHTTP(httptest.NewRecorder(), req)
	}
}
