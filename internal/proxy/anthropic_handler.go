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

// AnthropicHandler intercepts Anthropic POST /v1/messages streams.
type AnthropicHandler struct {
	upstream   *url.URL
	reverse    *httputil.ReverseProxy
	cache      *cache.Store
	compressor *compressor.StateTracker
	logger     *slog.Logger
	opts       Options
	pipeline   streamPipeline
}

// NewAnthropicHandler wires the reverse proxy for Anthropic message streams.
func NewAnthropicHandler(opts Options, store *cache.Store, logger *slog.Logger) (*AnthropicHandler, error) {
	u, err := url.Parse(opts.UpstreamURL)
	if err != nil {
		return nil, fmt.Errorf("anthropic proxy: invalid upstream URL: %w", err)
	}
	if logger == nil {
		logger = slog.Default()
	}

	h := &AnthropicHandler{
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
		logger.Error("anthropic upstream error", "err", err)
		opts.Metrics.RecordError("anthropic", "upstream")
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
	}

	originalDirector := rp.Director
	rp.Director = func(req *http.Request) {
		originalDirector(req)
		req.Host = u.Host
		req.URL.Host = u.Host
		req.URL.Scheme = u.Scheme
		forwardAuthHeaders(req)
		forwardAnthropicHeaders(req)
	}

	h.reverse = rp
	return h, nil
}

// NewAnthropicHandlerFromURL is a convenience wrapper for tests.
func NewAnthropicHandlerFromURL(upstreamURL string, store *cache.Store, logger *slog.Logger) (*AnthropicHandler, error) {
	return NewAnthropicHandler(Options{
		UpstreamURL:       upstreamURL,
		EnableCache:       true,
		EnableRedaction:   true,
		EnableCompression: true,
		CacheHitDelay:     2 * time.Millisecond,
	}, store, logger)
}

// ServeHTTP implements http.Handler for POST /v1/messages.
func (h *AnthropicHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	obs := newRequestObserver(h.opts.Metrics, "anthropic", "/v1/messages")
	defer obs.finish()

	if r.Method != http.MethodPost {
		h.opts.Metrics.RecordError("anthropic", "internal")
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	body, err := readLimitedBody(w, r, h.opts.MaxRequestBodyBytes)
	if err != nil {
		if isBodyLimitError(err) {
			h.opts.Metrics.RecordError("anthropic", "body_limit")
		} else {
			h.opts.Metrics.RecordError("anthropic", "parse")
		}
		return
	}
	h.opts.Metrics.RecordRequestBody("anthropic", len(body))

	req, err := models.ParseMessagesRequest(body)
	if err != nil {
		h.opts.Metrics.RecordError("anthropic", "parse")
		http.Error(w, "invalid json", http.StatusBadRequest)
		return
	}
	obs.setStream(req.Stream)

	scope, scopeMeta := h.opts.Scope.Resolve(r)
	recordScopeMetrics(h.opts.Metrics, scopeMeta)

	processed, cacheSource, redactionMap := h.applyAnthropicMiddleware(scope, req)
	recordRedactionMetrics(h.opts.Metrics, redactionMap)
	cacheKey := h.anthropicCacheKey(scope, cacheSource)

	if cacheKey != "" {
		if entry, err := h.cache.Get(cacheKey); err != nil {
			h.logger.Error("cache get failed", "key", cache.EntryID(cacheKey), "err", err)
			h.opts.Metrics.RecordError("anthropic", "internal")
		} else if entry != nil {
			obs.setCacheStatus("hit")
			h.opts.Metrics.RecordCacheHit("anthropic", len(entry.RawSSE))
			h.logger.Info("cache hit", "key", cache.EntryID(cacheKey), "format", StreamAnthropic)
			h.pipeline.replayCached(r.Context(), w, entry.RawSSE, redactionMap, StreamAnthropic)
			return
		}
		obs.setCacheStatus("miss")
		h.opts.Metrics.RecordCacheMiss("anthropic")
		h.logger.Info("cache miss", "key", cache.EntryID(cacheKey), "format", StreamAnthropic)
	}

	newBody, err := processed.Marshal()
	if err != nil {
		h.opts.Metrics.RecordError("anthropic", "internal")
		http.Error(w, "marshal", http.StatusInternalServerError)
		return
	}

	rctx := requestContext{
		cacheKey:     cacheKey,
		redactionMap: redactionMap,
		model:        processed.Model,
		streaming:    processed.Stream,
		format:       StreamAnthropic,
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
			h.opts.Metrics.RecordError("anthropic", "internal")
			http.Error(w, "streaming connection failure", http.StatusInternalServerError)
			return
		}
		out = bw
	}

	h.reverse.ServeHTTP(out, r)
}

func (h *AnthropicHandler) modifyResponse(resp *http.Response) error {
	if start, ok := resp.Request.Context().Value(ctxKeyUpstreamStart{}).(time.Time); ok {
		recordUpstreamMetrics(h.opts.Metrics, "anthropic", resp.StatusCode, start)
	}
	rctx, _ := resp.Request.Context().Value(ctxKeyRequest{}).(requestContext)
	return h.pipeline.interceptResponse(resp, rctx)
}
