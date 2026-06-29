package proxy

import (
	"bytes"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/guardrail"
	"github.com/kortolabs/proxy-engine/internal/models"
	"github.com/kortolabs/proxy-engine/internal/sse"
)

// StreamFormat identifies provider-specific SSE semantics.
type StreamFormat string

const (
	StreamOpenAI    StreamFormat = "openai"
	StreamAnthropic StreamFormat = "anthropic"
)

type requestContext struct {
	cacheKey     string
	redactionMap *guardrail.RedactionMap
	model        string
	streaming    bool
	format       StreamFormat
}

type ctxKeyRequest struct{}

type streamPipeline struct {
	cache  *cache.Store
	logger *slog.Logger
	opts   Options
}

func (p *streamPipeline) interceptResponse(resp *http.Response, rctx requestContext) error {
	if !rctx.streaming || rctx.cacheKey == "" {
		return nil
	}
	if !strings.Contains(resp.Header.Get("Content-Type"), "text/event-stream") {
		return nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil
	}

	pr, pw := io.Pipe()
	origBody := resp.Body
	resp.Body = pr

	go func() {
		defer pw.Close()
		defer origBody.Close()

		reader := sse.NewReader(origBody)
		var captured bytes.Buffer
		complete := false

		for {
			frame, err := reader.Next()
			if err == io.EOF {
				break
			}
			if err != nil {
				p.logger.Error("sse read error", "err", err, "format", rctx.format)
				break
			}

			if frameComplete(frame, rctx.format) {
				complete = true
			}

			captured.Write(frame.Bytes())

			clientFrame := frame
			if rctx.redactionMap.Len() > 0 {
				clientFrame = sse.TransformDataLine(frame, func(payload []byte) []byte {
					return restorePayload(payload, rctx.redactionMap, rctx.format)
				})
			}

			if err := sse.WriteFrame(pw, clientFrame); err != nil {
				return
			}
		}

		if complete {
			entry := cache.Entry{
				Key:       rctx.cacheKey,
				RawSSE:    captured.Bytes(),
				Model:     rctx.model,
				CreatedAt: time.Now().Unix(),
			}
			if err := p.cache.Put(entry); err != nil {
				p.logger.Error("cache put failed", "key", cache.EntryID(rctx.cacheKey), "err", err)
			} else {
				p.logger.Info("cache stored", "key", cache.EntryID(rctx.cacheKey), "bytes", len(entry.RawSSE), "format", rctx.format)
			}
		}
	}()

	return nil
}

func (p *streamPipeline) replayCached(w http.ResponseWriter, raw []byte, rm *guardrail.RedactionMap, format StreamFormat) {
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("X-KortoLabs-Cache", "HIT")
	w.WriteHeader(http.StatusOK)

	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}

	reader := sse.NewReader(bytes.NewReader(raw))
	for {
		frame, err := reader.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			p.logger.Error("cache replay error", "err", err)
			break
		}

		out := frame
		if rm != nil && rm.Len() > 0 {
			out = sse.TransformDataLine(frame, func(payload []byte) []byte {
				return restorePayload(payload, rm, format)
			})
		}

		_ = sse.WriteFrame(w, out)
		flusher.Flush()

		if p.opts.CacheHitDelay > 0 {
			time.Sleep(p.opts.CacheHitDelay)
		}
	}
}

func frameComplete(frame sse.Frame, format StreamFormat) bool {
	switch format {
	case StreamAnthropic:
		return frame.IsAnthropicComplete()
	default:
		return frame.IsDone()
	}
}

func restorePayload(payload []byte, rm *guardrail.RedactionMap, format StreamFormat) []byte {
	switch format {
	case StreamAnthropic:
		return restoreAnthropicDelta(payload, rm)
	default:
		return restoreOpenAIChunk(payload, rm)
	}
}

func restoreOpenAIChunk(payload []byte, rm *guardrail.RedactionMap) []byte {
	var chunk models.StreamChunk
	if err := json.Unmarshal(payload, &chunk); err != nil {
		return payload
	}
	for i := range chunk.Choices {
		if chunk.Choices[i].Delta.Content != "" {
			chunk.Choices[i].Delta.Content = rm.Restore(chunk.Choices[i].Delta.Content)
		}
	}
	out, err := json.Marshal(chunk)
	if err != nil {
		return payload
	}
	return out
}

func restoreAnthropicDelta(payload []byte, rm *guardrail.RedactionMap) []byte {
	var evt models.AnthropicDeltaEvent
	if err := json.Unmarshal(payload, &evt); err != nil {
		return payload
	}
	if evt.Type != "content_block_delta" || evt.Delta.Text == "" {
		return payload
	}
	evt.Delta.Text = rm.Restore(evt.Delta.Text)
	out, err := json.Marshal(evt)
	if err != nil {
		return payload
	}
	return out
}
