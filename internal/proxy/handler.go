// Package proxy implements the core reverse-proxy handler with SSE interception,
// semantic cache lookup/store, and streaming passthrough.
package proxy

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httputil"
	"net/url"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/compressor"
	"github.com/kortolabs/proxy-engine/internal/models"
)

// Handler intercepts OpenAI-compatible POST /v1/chat/completions streams.
type Handler struct {
	upstream   *url.URL
	reverse    *httputil.ReverseProxy
	cache      *cache.Store
	compressor *compressor.StateTracker
	logger     *slog.Logger
	opts       Options
	pipeline   streamPipeline
}

// NewHandler wires the reverse proxy to the upstream base URL and cache store.
func NewHandler(opts Options, store *cache.Store, logger *slog.Logger) (*Handler, error) {
	u, err := url.Parse(opts.UpstreamURL)
	if err != nil {
		return nil, fmt.Errorf("proxy: invalid upstream URL: %w", err)
	}
	if logger == nil {
		logger = slog.Default()
	}

	h := &Handler{
		upstream:   u,
		cache:      store,
		compressor: compressor.NewStateTracker(opts.CompressorMaxScopes, opts.CompressorScopeTTL),
		logger:     logger,
		opts:       opts,
		pipeline:   streamPipeline{cache: store, logger: logger, opts: opts},
	}

	rp := httputil.NewSingleHostReverseProxy(u)
	rp.FlushInterval = -1
	rp.ModifyResponse = h.modifyResponse
	rp.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, err error) {
		logger.Error("upstream error", "err", err)
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
	}

	originalDirector := rp.Director
	rp.Director = func(req *http.Request) {
		originalDirector(req)
		req.Host = u.Host
		req.URL.Host = u.Host
		req.URL.Scheme = u.Scheme
		forwardAuthHeaders(req)
	}

	h.reverse = rp
	return h, nil
}

// NewHandlerFromURL is a convenience wrapper for tests.
func NewHandlerFromURL(upstreamURL string, store *cache.Store, logger *slog.Logger) (*Handler, error) {
	return NewHandler(Options{
		UpstreamURL:         upstreamURL,
		EnableCache:           true,
		EnableRedaction:       true,
		EnableCompression:     true,
		CacheHitDelay:         2 * time.Millisecond,
		CompressorMaxScopes:   10_000,
		CompressorScopeTTL:    time.Hour,
	}, store, logger)
}

// ServeHTTP implements http.Handler for POST /v1/chat/completions.
func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	body, err := readLimitedBody(w, r, h.opts.MaxRequestBodyBytes)
	if err != nil {
		return
	}

	req, err := models.ParseChatCompletionRequest(body)
	if err != nil {
		http.Error(w, "invalid json", http.StatusBadRequest)
		return
	}

	scope := h.opts.Scope.FromRequest(r)
	processed, cacheSource, redactionMap := h.applyOpenAIMiddleware(scope, req)
	cacheKey := h.openAICacheKey(scope, cacheSource)

	if cacheKey != "" {
		if entry, err := h.cache.Get(cacheKey); err != nil {
			h.logger.Error("cache get failed", "key", cache.EntryID(cacheKey), "err", err)
		} else if entry != nil {
			h.logger.Info("cache hit", "key", cache.EntryID(cacheKey), "format", StreamOpenAI)
			h.pipeline.replayCached(r.Context(), w, entry.RawSSE, redactionMap, StreamOpenAI)
			return
		}
	}

	if cacheKey != "" {
		h.logger.Info("cache miss", "key", cache.EntryID(cacheKey), "format", StreamOpenAI)
	}

	newBody, err := processed.Marshal()
	if err != nil {
		http.Error(w, "marshal", http.StatusInternalServerError)
		return
	}

	rctx := requestContext{
		cacheKey:     cacheKey,
		redactionMap: redactionMap,
		model:        processed.Model,
		streaming:    processed.Stream,
		format:       StreamOpenAI,
	}
	ctx := context.WithValue(r.Context(), ctxKeyRequest{}, rctx)
	r = r.WithContext(ctx)
	r.Body = io.NopCloser(bytes.NewReader(newBody))
	r.ContentLength = int64(len(newBody))
	r.Header.Set("Content-Type", "application/json")

	out := w
	if processed.Stream {
		bw, err := bootstrapUpstreamSSE(w, h.logger)
		if err != nil {
			h.logger.Error("sse bootstrap failed", "err", err)
			http.Error(w, "streaming connection failure", http.StatusInternalServerError)
			return
		}
		out = bw
	}

	h.reverse.ServeHTTP(out, r)
}

func (h *Handler) modifyResponse(resp *http.Response) error {
	rctx, _ := resp.Request.Context().Value(ctxKeyRequest{}).(requestContext)
	return h.pipeline.interceptResponse(resp, rctx)
}
