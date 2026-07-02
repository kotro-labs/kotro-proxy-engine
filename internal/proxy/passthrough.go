package proxy

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"net/http/httputil"
	"net/url"
	"time"

	"github.com/kortolabs/proxy-engine/internal/metrics"
)

// Passthrough forwards all other /v1/* requests to the upstream provider unchanged.
type Passthrough struct {
	reverse *httputil.ReverseProxy
	logger  *slog.Logger
	metrics *metrics.Registry
}

// NewPassthrough creates a generic reverse proxy for models, embeddings, etc.
func NewPassthrough(upstreamURL string, logger *slog.Logger, m *metrics.Registry) (*Passthrough, error) {
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
	rp.ModifyResponse = func(resp *http.Response) error {
		if start, ok := resp.Request.Context().Value(ctxKeyUpstreamStart{}).(time.Time); ok {
			recordUpstreamMetrics(m, "passthrough", resp.StatusCode, start)
		}
		return nil
	}
	rp.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, err error) {
		logger.Error("passthrough upstream error", "err", err)
		m.RecordError("passthrough", "upstream")
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
	}

	return &Passthrough{reverse: rp, logger: logger, metrics: m}, nil
}

// ServeHTTP implements http.Handler.
func (p *Passthrough) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	obs := newRequestObserver(p.metrics, "passthrough", r.URL.Path)
	defer obs.finish()

	ctx := r.Context()
	if r.Body != nil {
		ctx = contextWithUpstreamStart(ctx)
	}
	r = r.WithContext(ctx)

	p.reverse.ServeHTTP(w, r)
}

func contextWithUpstreamStart(ctx context.Context) context.Context {
	return context.WithValue(ctx, ctxKeyUpstreamStart{}, time.Now())
}
