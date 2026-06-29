package proxy

import (
	"fmt"
	"log/slog"
	"net/http"
	"net/http/httputil"
	"net/url"
)

// Passthrough forwards all other /v1/* requests to the upstream provider unchanged.
type Passthrough struct {
	reverse *httputil.ReverseProxy
	logger  *slog.Logger
}

// NewPassthrough creates a generic reverse proxy for models, embeddings, etc.
func NewPassthrough(upstreamURL string, logger *slog.Logger) (*Passthrough, error) {
	u, err := url.Parse(upstreamURL)
	if err != nil {
		return nil, fmt.Errorf("passthrough: invalid upstream URL: %w", err)
	}
	if logger == nil {
		logger = slog.Default()
	}

	rp := httputil.NewSingleHostReverseProxy(u)
	originalDirector := rp.Director
	rp.Director = func(req *http.Request) {
		originalDirector(req)
		req.Host = u.Host
		req.URL.Host = u.Host
		req.URL.Scheme = u.Scheme
		forwardAuthHeaders(req)
	}
	rp.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, err error) {
		logger.Error("passthrough upstream error", "err", err)
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
	}

	return &Passthrough{reverse: rp, logger: logger}, nil
}

// ServeHTTP implements http.Handler.
func (p *Passthrough) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	p.reverse.ServeHTTP(w, r)
}
