// Package dashboard serves the local developer observability UI.
package dashboard

import (
	_ "embed"
	"encoding/json"
	"net/http"

	"github.com/kortolabs/proxy-engine/internal/metrics"
)

//go:embed page.html
var pageHTML []byte

// Handler serves dashboard routes when metrics are enabled.
type Handler struct {
	metrics *metrics.Registry
}

// New returns a dashboard handler backed by the metrics registry.
func New(m *metrics.Registry) *Handler {
	return &Handler{metrics: m}
}

// Register mounts /dashboard and /api/dashboard on mux.
func (h *Handler) Register(mux *http.ServeMux) {
	if h == nil || h.metrics == nil {
		return
	}
	mux.HandleFunc("/dashboard", h.servePage)
	mux.HandleFunc("/api/dashboard", h.serveAPI)
}

func (h *Handler) servePage(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(pageHTML)
}

func (h *Handler) serveAPI(w http.ResponseWriter, _ *http.Request) {
	snap := h.metrics.Snapshot()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	_ = enc.Encode(snap)
}
