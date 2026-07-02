package server_test

import (
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/server"
)

func TestMetricsEndpointEnabledByDefault(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	defer upstream.Close()

	cfg := config.Config{
		ListenAddr:  ":0",
		UpstreamURL: upstream.URL,
		CacheDBPath: t.TempDir() + "/cache.db",
		EnableMetrics: true,
	}
	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = srv.Shutdown(t.Context()) })

	rr := httptest.NewRecorder()
	srv.HTTPHandler().ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/metrics", nil))
	if rr.Code != http.StatusOK {
		t.Fatalf("metrics status %d", rr.Code)
	}
	if !strings.Contains(rr.Body.String(), "korto_cache_entries") {
		t.Fatalf("expected prometheus exposition, got: %s", rr.Body.String())
	}
}

func TestMetricsEndpointDisabled(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	defer upstream.Close()

	cfg := config.Config{
		ListenAddr:    ":0",
		UpstreamURL:   upstream.URL,
		CacheDBPath:   t.TempDir() + "/cache.db",
		EnableMetrics: false,
	}
	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = srv.Shutdown(t.Context()) })

	rr := httptest.NewRecorder()
	srv.HTTPHandler().ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/metrics", nil))
	if rr.Code != http.StatusNotFound {
		t.Fatalf("expected 404 when metrics disabled, got %d", rr.Code)
	}
}
