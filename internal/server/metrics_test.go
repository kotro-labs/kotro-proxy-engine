package server_test

import (
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/server"
)

func TestMetricsEndpointEnabledByDefault(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	defer upstream.Close()

	cfg := config.Config{
		ListenAddr:    ":0",
		MetricsAddr:   ":0",
		UpstreamURL:   upstream.URL,
		CacheDBPath:   t.TempDir() + "/cache.db",
		EnableMetrics: true,
	}
	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = srv.Shutdown(t.Context()) })

	rr := httptest.NewRecorder()
	srv.MetricsHTTPHandler().ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/metrics", nil))
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
	srv.MetricsHTTPHandler().ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/metrics", nil))
	if rr.Code != http.StatusNotFound {
		t.Fatalf("expected 404 when metrics disabled, got %d", rr.Code)
	}
}

func TestMetricsNotExposedOnProxySocket(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	defer upstream.Close()

	cfg := config.Config{
		ListenAddr:    ":0",
		MetricsAddr:   ":0",
		UpstreamURL:   upstream.URL,
		CacheDBPath:   t.TempDir() + "/cache.db",
		EnableMetrics: true,
	}
	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = srv.Shutdown(t.Context()) })

	rr := httptest.NewRecorder()
	srv.HTTPHandler().ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/metrics", nil))
	if rr.Code != http.StatusNotFound {
		t.Fatalf("expected /metrics absent on proxy socket, got status %d", rr.Code)
	}
}

func TestNetworkSocket_StrictSegregation(t *testing.T) {
	proxyLn, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	proxyAddr := proxyLn.Addr().String()
	_ = proxyLn.Close()

	metricsLn, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	metricsAddr := metricsLn.Addr().String()
	_ = metricsLn.Close()

	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	defer upstream.Close()

	cfg := config.Config{
		ListenAddr:    proxyAddr,
		MetricsAddr:   metricsAddr,
		UpstreamURL:   upstream.URL,
		CacheDBPath:   t.TempDir() + "/cache.db",
		EnableMetrics: true,
	}
	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = srv.Shutdown(t.Context()) })

	errCh := make(chan error, 1)
	go func() {
		errCh <- srv.ListenAndServe()
	}()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if resp, err := http.Get("http://" + metricsAddr + "/metrics"); err == nil {
			_ = resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				break
			}
		}
		time.Sleep(20 * time.Millisecond)
	}

	resp, err := http.Get("http://" + proxyAddr + "/metrics")
	if err == nil {
		defer resp.Body.Close()
		if resp.StatusCode == http.StatusOK {
			t.Fatal("SECURITY: /metrics reachable on proxy gateway socket")
		}
	}

	resp, err = http.Get("http://" + metricsAddr + "/metrics")
	if err != nil {
		t.Fatalf("telemetry /metrics unreachable on isolated socket: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("telemetry /metrics status %d", resp.StatusCode)
	}

	_ = srv.Shutdown(t.Context())
	select {
	case err := <-errCh:
		if err != nil {
			t.Log("listener exit:", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("timed out waiting for server shutdown")
	}
}
