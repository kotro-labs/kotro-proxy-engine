package proxy

import (
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/guardrail"
	"github.com/kortolabs/proxy-engine/internal/models"
)

// Options configures the chat-completions interceptor pipeline.
type Options struct {
	UpstreamURL       string
	EnableCache       bool
	EnableRedaction   bool
	EnableCompression bool
	CacheHitDelay     time.Duration
}

// OptionsFromConfig maps application config to proxy options.
func OptionsFromConfig(cfg config.Config) Options {
	return Options{
		UpstreamURL:       cfg.UpstreamURL,
		EnableCache:       cfg.EnableCache,
		EnableRedaction:   cfg.EnableRedaction,
		EnableCompression: cfg.EnableCompression,
		CacheHitDelay:     cfg.CacheHitDelay,
	}
}

func (h *Handler) applyOpenAIMiddleware(req *models.ChatCompletionRequest) (processed, cacheSource *models.ChatCompletionRequest, rm *guardrail.RedactionMap) {
	out := req.Clone()

	if h.opts.EnableRedaction {
		out, rm = guardrail.RedactRequest(out)
	} else {
		rm = guardrail.NewRedactionMap()
	}

	cacheSource = out.Clone()

	if h.opts.EnableCompression {
		out = h.compressor.CompressRequest(out)
	}

	return out, cacheSource, rm
}

func (h *Handler) openAICacheKey(req *models.ChatCompletionRequest) string {
	if !h.opts.EnableCache || !req.Stream {
		return ""
	}
	systemPrompt, latestUser := req.ExtractPromptState()
	return cache.KeyForRequest(systemPrompt, latestUser, req.Model, string(StreamOpenAI))
}

func (h *AnthropicHandler) applyAnthropicMiddleware(req *models.MessagesRequest) (processed, cacheSource *models.MessagesRequest, rm *guardrail.RedactionMap) {
	out := req.Clone()

	if h.opts.EnableRedaction {
		out, rm = guardrail.RedactAnthropicRequest(out)
	} else {
		rm = guardrail.NewRedactionMap()
	}

	cacheSource = out.Clone()

	if h.opts.EnableCompression {
		out = h.compressor.CompressAnthropicRequest(out)
	}

	return out, cacheSource, rm
}

func (h *AnthropicHandler) anthropicCacheKey(req *models.MessagesRequest) string {
	if !h.opts.EnableCache || !req.Stream {
		return ""
	}
	systemPrompt, latestUser := req.ExtractPromptState()
	return cache.KeyForRequest(systemPrompt, latestUser, req.Model, string(StreamAnthropic))
}
