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
	"github.com/kortolabs/proxy-engine/internal/optimizer"
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
		compressor: compressor.NewStateTracker(opts.CompressorMaxScopes, opts.CompressorScopeTTL, opts.Metrics),
		logger:     logger,
		opts:       opts,
		pipeline:   streamPipeline{cache: store, logger: logger, opts: opts},
	}

	rp := httputil.NewSingleHostReverseProxy(u)
	rp.FlushInterval = -1
	rp.ModifyResponse = h.modifyResponse
	rp.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, err error) {
		logger.Error("upstream error", "err", err)
		opts.Metrics.RecordError("openai", "upstream")
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
	}

	var fallback *url.URL
	if opts.FallbackURL != "" {
		f, err := url.Parse(opts.FallbackURL)
		if err != nil {
			logger.Warn("invalid fallback URL in handler options; failover disabled", "err", err)
		} else {
			fallback = f
		}
	}

	rp.Transport = &failoverTransport{
		next:        http.DefaultTransport,
		fallbackURL: fallback,
		logger:      logger,
		metrics:     opts.Metrics,
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
		UpstreamURL:           upstreamURL,
		EnableCache:           true,
		EnableRedaction:       true,
		EnableCompression:     true,
		CacheHitDelay:         2 * time.Millisecond,
		CompressorMaxScopes:   10_000,
		CompressorScopeTTL:    time.Hour,
		CacheKeyStrategy:      cache.StrategyWindowN,
		CacheWindowSize:       4,
	}, store, logger)
}

// ServeHTTP implements http.Handler for POST /v1/chat/completions.
func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	obs := newRequestObserver(h.opts.Metrics, "openai", "/v1/chat/completions")
	defer obs.finish()

	if r.Method != http.MethodPost {
		h.opts.Metrics.RecordError("openai", "internal")
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	body, err := readLimitedBody(w, r, h.opts.MaxRequestBodyBytes)
	if err != nil {
		if isBodyLimitError(err) {
			h.opts.Metrics.RecordError("openai", "body_limit")
		} else {
			h.opts.Metrics.RecordError("openai", "parse")
		}
		return
	}
	h.opts.Metrics.RecordRequestBody("openai", len(body))

	req, err := models.ParseChatCompletionRequest(body)
	if err != nil {
		h.opts.Metrics.RecordError("openai", "parse")
		http.Error(w, "invalid json", http.StatusBadRequest)
		return
	}
	obs.setStream(req.Stream)

	// Intercept and restructure the payload to the DeepSeek server-side caching matrix
	optimizer.EnforceCacheMatrix(req)

	scope, scopeMeta := h.opts.Scope.Resolve(r)
	recordScopeMetrics(h.opts.Metrics, scopeMeta)

	processed, cacheSource, redactionMap := h.applyOpenAIMiddleware(scope, req)
	recordRedactionMetrics(h.opts.Metrics, redactionMap)
	cacheKey := h.openAICacheKey(scope, cacheSource)

	if cacheKey != "" {
		if entry, err := h.cache.Get(cacheKey); err != nil {
			h.logger.Error("cache get failed", "key", cache.EntryID(cacheKey), "err", err)
			h.opts.Metrics.RecordError("openai", "internal")
		} else if entry != nil {
			obs.setCacheStatus("hit")
			h.opts.Metrics.RecordCacheHit("openai", len(entry.RawSSE))
			h.logger.Info("cache hit", "key", cache.EntryID(cacheKey), "format", StreamOpenAI)
			h.pipeline.replayCached(r.Context(), w, entry.RawSSE, redactionMap, StreamOpenAI)
			return
		}
		obs.setCacheStatus("miss")
		h.opts.Metrics.RecordCacheMiss("openai")
		h.logger.Info("cache miss", "key", cache.EntryID(cacheKey), "format", StreamOpenAI)
	}

	newBody, err := processed.Marshal()
	if err != nil {
		h.opts.Metrics.RecordError("openai", "internal")
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
	ctx = context.WithValue(ctx, ctxKeyUpstreamStart{}, time.Now())
	r = r.WithContext(ctx)
	r.Body = io.NopCloser(bytes.NewReader(newBody))
	r.ContentLength = int64(len(newBody))
	r.Header.Set("Content-Type", "application/json")

	out := w
	if processed.Stream {
		bw, err := bootstrapUpstreamSSE(w, h.logger)
		if err != nil {
			h.logger.Error("sse bootstrap failed", "err", err)
			h.opts.Metrics.RecordError("openai", "internal")
			http.Error(w, "streaming connection failure", http.StatusInternalServerError)
			return
		}
		out = bw
	}

	h.reverse.ServeHTTP(out, r)
}

func (h *Handler) modifyResponse(resp *http.Response) error {
	if start, ok := resp.Request.Context().Value(ctxKeyUpstreamStart{}).(time.Time); ok {
		recordUpstreamMetrics(h.opts.Metrics, "openai", resp.StatusCode, start)
	}
	rctx, _ := resp.Request.Context().Value(ctxKeyRequest{}).(requestContext)
	return h.pipeline.interceptResponse(resp, rctx)
}
