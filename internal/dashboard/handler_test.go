package dashboard_test

import (
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/server"
)

func TestDashboardPageAndAPI(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("data: [DONE]\n\n"))
	}))
	defer upstream.Close()

	cfg := config.Config{
		ListenAddr:    ":0",
		UpstreamURL:   upstream.URL,
		CacheDBPath:   t.TempDir() + "/cache.db",
		EnableMetrics: true,
	}
	srv, err := server.New(cfg, slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = srv.Shutdown(t.Context()) })

	page := httptest.NewRecorder()
	srv.MetricsHTTPHandler().ServeHTTP(page, httptest.NewRequest(http.MethodGet, "/dashboard", nil))
	if page.Code != http.StatusOK {
		t.Fatalf("dashboard page status %d", page.Code)
	}
	if !strings.Contains(page.Body.String(), "Kotro Proxy Dashboard") {
		t.Fatal("expected dashboard HTML")
	}

	api := httptest.NewRecorder()
	srv.MetricsHTTPHandler().ServeHTTP(api, httptest.NewRequest(http.MethodGet, "/api/dashboard", nil))
	if api.Code != http.StatusOK {
		t.Fatalf("dashboard api status %d", api.Code)
	}
	var payload struct {
		RecentRequests []any `json:"recent_requests"`
	}
	if err := json.Unmarshal(api.Body.Bytes(), &payload); err != nil {
		t.Fatal(err)
	}
}
