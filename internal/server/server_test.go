package server_test

import (
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/server"
)

func TestPprofEnabled(t *testing.T) {
	cfg := config.Load()
	cfg.EnablePprof = true
	cfg.EnableMetrics = true
	cfg.CacheDBPath = t.TempDir() + "/cache.db"

	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/debug/pprof/goroutine?debug=1", nil)
	rec := httptest.NewRecorder()
	srv.MetricsHTTPHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("pprof status %d", rec.Code)
	}
	if !contains(rec.Body.String(), "goroutine profile") {
		t.Fatalf("expected goroutine profile output, got: %s", rec.Body.String()[:min(120, rec.Body.Len())])
	}
}

func TestPprofOnProxyWhenMetricsDisabled(t *testing.T) {
	cfg := config.Load()
	cfg.EnablePprof = true
	cfg.EnableMetrics = false
	cfg.CacheDBPath = t.TempDir() + "/cache.db"

	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/debug/pprof/goroutine?debug=1", nil)
	rec := httptest.NewRecorder()
	srv.HTTPHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("pprof status %d", rec.Code)
	}
}

func contains(s, sub string) bool {
	return len(s) >= len(sub) && (s == sub || len(sub) == 0 || indexOf(s, sub) >= 0)
}

func indexOf(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
