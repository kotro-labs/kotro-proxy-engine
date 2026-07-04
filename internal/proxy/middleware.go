package proxy

import (
	"log/slog"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/compressor"
	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/guardrail"
	"github.com/kortolabs/proxy-engine/internal/metrics"
	"github.com/kortolabs/proxy-engine/internal/models"
	"time"
)

// Options configures the chat-completions interceptor pipeline.
type Options struct {
	UpstreamURL         string
	FallbackURL         string
	EnableCache         bool
	EnableRedaction     bool
	EnableCompression   bool
	CacheHitDelay       time.Duration
	MaxRequestBodyBytes int64
	Scope               ScopeResolver
	CompressorMaxScopes int
	CompressorScopeTTL  time.Duration
	Metrics             *metrics.Registry
	CacheKeyStrategy    cache.CacheKeyStrategy
	CacheWindowSize     int
}

// OptionsFromConfig maps application config to proxy options.
func OptionsFromConfig(cfg config.Config, logger *slog.Logger, m *metrics.Registry) Options {
	cidrs, err := parseTrustedCIDRs(cfg.TrustedProxyCIDRs)
	if err != nil {
		if logger == nil {
			logger = slog.Default()
		}
		logger.Error(
			"invalid KORTO_TRUSTED_PROXY_CIDRS; failing safe with empty trusted-proxy whitelist",
			"err", err,
			"value", cfg.TrustedProxyCIDRs,
		)
		cidrs = nil
	}

	return Options{
		UpstreamURL:         cfg.UpstreamURL,
		FallbackURL:         cfg.FallbackURL,
		EnableCache:         cfg.EnableCache,
		EnableRedaction:     cfg.EnableRedaction,
		EnableCompression:   cfg.EnableCompression,
		CacheHitDelay:       cfg.CacheHitDelay,
		MaxRequestBodyBytes: cfg.MaxRequestBodyBytes,
		Scope: ScopeResolver{
			TrustUpstreamGateway: cfg.TrustUpstreamGateway,
			TrustedProxyCIDRs:    cidrs,
		},
		CompressorMaxScopes: cfg.CompressorMaxScopes,
		CompressorScopeTTL:  cfg.CompressorScopeTTL,
		Metrics:             m,
		CacheKeyStrategy:    cfg.CacheKeyStrategy,
		CacheWindowSize:     cfg.CacheWindowSize,
	}
}

func (h *Handler) applyOpenAIMiddleware(scope compressor.Scope, req *models.ChatCompletionRequest) (processed, cacheSource *models.ChatCompletionRequest, rm *guardrail.RedactionMap) {
	out := req.Clone()

	if h.opts.EnableRedaction {
		out, rm = guardrail.RedactRequest(out)
	} else {
		rm = guardrail.NewRedactionMap()
	}

	cacheSource = out.Clone()

	if h.opts.EnableCompression {
		out = h.compressor.CompressRequest(scope, out)
	}

	return out, cacheSource, rm
}

func (h *Handler) openAICacheKey(scope compressor.Scope, req *models.ChatCompletionRequest) string {
	if !h.opts.EnableCache || !req.Stream {
		return ""
	}
	material := req.ExtractCacheKeyMaterial(h.opts.CacheKeyStrategy, h.opts.CacheWindowSize)
	return cache.KeyForRequestWithStrategy(scope.Key(), req.Model, string(StreamOpenAI), material)
}

func (h *AnthropicHandler) applyAnthropicMiddleware(scope compressor.Scope, req *models.MessagesRequest) (processed, cacheSource *models.MessagesRequest, rm *guardrail.RedactionMap) {
	out := req.Clone()

	if h.opts.EnableRedaction {
		out, rm = guardrail.RedactAnthropicRequest(out)
	} else {
		rm = guardrail.NewRedactionMap()
	}

	cacheSource = out.Clone()

	if h.opts.EnableCompression {
		out = h.compressor.CompressAnthropicRequest(scope, out)
	}

	return out, cacheSource, rm
}

func (h *AnthropicHandler) anthropicCacheKey(scope compressor.Scope, req *models.MessagesRequest) string {
	if !h.opts.EnableCache || !req.Stream {
		return ""
	}
	material := req.ExtractCacheKeyMaterial(h.opts.CacheKeyStrategy, h.opts.CacheWindowSize)
	return cache.KeyForRequestWithStrategy(scope.Key(), req.Model, string(StreamAnthropic), material)
}
