package proxy

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/guardrail"
	"github.com/kortolabs/proxy-engine/internal/metrics"
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
	ctx := resp.Request.Context()

	startPipeWatchdog(ctx, pw, origBody, p.logger)

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
				if ctx.Err() != nil {
					p.logger.Debug("stream interception stopped", "err", ctx.Err(), "format", rctx.format)
				} else {
					p.logger.Error("sse read error", "err", err, "format", rctx.format)
				}
				break
			}

			if frameComplete(frame, rctx.format) {
				complete = true
			}

			captured.Write(frame.Bytes())

			clientFrame := frame
			if rctx.redactionMap.Len() > 0 {
				clientFrame = sse.TransformDataLine(frame, func(payload []byte) []byte {
					out, restores := restorePayloadCounted(payload, rctx.redactionMap, rctx.format)
					p.opts.Metrics.RecordRedactionRestores(restores)
					return out
				})
			}

			if err := sse.WriteFrame(pw, clientFrame); err != nil {
				p.logger.Debug("stream interception stopped: pipe writer closed", "err", err, "format", rctx.format)
				return
			}
		}

		if complete && ctx.Err() == nil {
			entry := cache.Entry{
				Key:       rctx.cacheKey,
				RawSSE:    captured.Bytes(),
				Model:     rctx.model,
				CreatedAt: time.Now().Unix(),
			}
			if err := p.cache.Put(entry); err != nil {
				p.logger.Error("cache put failed", "key", cache.EntryID(rctx.cacheKey), "err", err)
			} else {
				provider := metrics.ProviderLabel(string(rctx.format))
				p.opts.Metrics.RecordCacheStore(provider)
				if n, err := p.cache.Count(); err == nil {
					p.opts.Metrics.SetCacheEntries(n)
				}
				p.logger.Info("cache stored", "key", cache.EntryID(rctx.cacheKey), "bytes", len(entry.RawSSE), "format", rctx.format)
			}
		}
	}()

	return nil
}

// startPipeWatchdog closes the interception pipe when the request context ends.
// CloseWithError unblocks any goroutine stuck in pw.Write; closing upstream releases
// blocked Read calls in the SSE translation loop.
func startPipeWatchdog(ctx context.Context, pw *io.PipeWriter, upstream io.ReadCloser, logger *slog.Logger) {
	go func() {
		<-ctx.Done()
		err := ctx.Err()
		if err == nil {
			err = context.Canceled
		}
		if closeErr := pw.CloseWithError(err); closeErr != nil && !errors.Is(closeErr, io.ErrClosedPipe) {
			logger.Debug("pipe watchdog close", "err", closeErr)
		}
		if upstream != nil {
			_ = upstream.Close()
		}
	}()
}

func (p *streamPipeline) replayCached(ctx context.Context, w http.ResponseWriter, raw []byte, rm *guardrail.RedactionMap, format StreamFormat) error {
	if err := ctx.Err(); err != nil {
		return err
	}

	setSSEHeaders(w)
	w.Header().Set("X-KortoLabs-Cache", "HIT")
	w.WriteHeader(http.StatusOK)

	if _, err := w.Write([]byte(sseBootstrapComment)); err != nil {
		p.logger.Debug("cache replay bootstrap write failed", "err", err)
		return err
	}
	if err := flushResponse(w); err != nil {
		p.logger.Debug("cache replay bootstrap flush failed", "err", err)
		return err
	}

	reader := sse.NewReader(bytes.NewReader(raw))
	for {
		if err := ctx.Err(); err != nil {
			p.logger.Debug("cache replay aborted: client disconnected", "err", err, "format", format)
			return err
		}

		frame, err := reader.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			p.logger.Error("cache replay error", "err", err)
			return err
		}

		out := frame
		if rm != nil && rm.Len() > 0 {
			out = sse.TransformDataLine(frame, func(payload []byte) []byte {
				restored, restores := restorePayloadCounted(payload, rm, format)
				p.opts.Metrics.RecordRedactionRestores(restores)
				return restored
			})
		}

		if err := sse.WriteFrame(w, out); err != nil {
			p.logger.Debug("cache replay failed: write error", "err", err, "format", format)
			return err
		}
		if err := flushResponse(w); err != nil {
			p.logger.Debug("cache replay failed: network flush error", "err", err, "format", format)
			return err
		}

		if p.opts.CacheHitDelay > 0 {
			select {
			case <-ctx.Done():
				p.logger.Debug("cache replay aborted during pacing", "err", ctx.Err(), "format", format)
				return ctx.Err()
			case <-time.After(p.opts.CacheHitDelay):
			}
		}
	}

	return nil
}

func frameComplete(frame sse.Frame, format StreamFormat) bool {
	switch format {
	case StreamAnthropic:
		return frame.IsAnthropicComplete()
	default:
		return frame.IsDone()
	}
}

func restorePayloadCounted(payload []byte, rm *guardrail.RedactionMap, format StreamFormat) ([]byte, int) {
	switch format {
	case StreamAnthropic:
		return restoreAnthropicDeltaCounted(payload, rm)
	default:
		return restoreOpenAIChunkCounted(payload, rm)
	}
}

func restorePayload(payload []byte, rm *guardrail.RedactionMap, format StreamFormat) []byte {
	out, _ := restorePayloadCounted(payload, rm, format)
	return out
}

func restoreOpenAIChunkCounted(payload []byte, rm *guardrail.RedactionMap) ([]byte, int) {
	var chunk models.StreamChunk
	if err := json.Unmarshal(payload, &chunk); err != nil {
		return payload, 0
	}
	restores := 0
	for i := range chunk.Choices {
		if chunk.Choices[i].Delta.Content != "" {
			var n int
			chunk.Choices[i].Delta.Content, n = rm.RestoreCounted(chunk.Choices[i].Delta.Content)
			restores += n
		}
	}
	out, err := json.Marshal(chunk)
	if err != nil {
		return payload, 0
	}
	return out, restores
}

func restoreOpenAIChunk(payload []byte, rm *guardrail.RedactionMap) []byte {
	out, _ := restoreOpenAIChunkCounted(payload, rm)
	return out
}

func restoreAnthropicDeltaCounted(payload []byte, rm *guardrail.RedactionMap) ([]byte, int) {
	var evt models.AnthropicDeltaEvent
	if err := json.Unmarshal(payload, &evt); err != nil {
		return payload, 0
	}
	if evt.Type != "content_block_delta" || evt.Delta.Text == "" {
		return payload, 0
	}
	var n int
	evt.Delta.Text, n = rm.RestoreCounted(evt.Delta.Text)
	restores := n
	out, err := json.Marshal(evt)
	if err != nil {
		return payload, 0
	}
	return out, restores
}

func restoreAnthropicDelta(payload []byte, rm *guardrail.RedactionMap) []byte {
	out, _ := restoreAnthropicDeltaCounted(payload, rm)
	return out
}
