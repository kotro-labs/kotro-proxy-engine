// Package guardrail implements the local privacy guardrail (Feature B).
package guardrail

import (
	"regexp"
	"strings"
	"sync"

	"github.com/kortolabs/proxy-engine/internal/models"
)

// RedactionMap holds placeholder -> original value mappings for a single request.
type RedactionMap struct {
	mu      sync.RWMutex
	forward map[string]string
	reverse map[string]string
	seq     int
}

// NewRedactionMap creates an empty per-request redaction registry.
func NewRedactionMap() *RedactionMap {
	return &RedactionMap{
		forward: make(map[string]string),
		reverse: make(map[string]string),
	}
}

var sensitivePatterns = []*regexp.Regexp{
	regexp.MustCompile(`AKIA[0-9A-Z]{16}`),
	regexp.MustCompile(`(?i)(?:password|passwd|pwd)\s*[:=]\s*['"]?[^\s'"]{4,}['"]?`),
	regexp.MustCompile(`(?i)(?:api[_-]?key|secret[_-]?key|token)\s*[:=]\s*['"]?[^\s'"]{8,}['"]?`),
	regexp.MustCompile(`postgres(?:ql)?://[^\s]+`),
	regexp.MustCompile(`mysql://[^\s]+`),
	regexp.MustCompile(`mongodb(?:\+srv)?://[^\s]+`),
	regexp.MustCompile(`redis://[^\s]+`),
	regexp.MustCompile(`[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}`),
	regexp.MustCompile(`sk-[a-zA-Z0-9]{20,}`),
	regexp.MustCompile(`sk-ant-[a-zA-Z0-9\-]{20,}`),
}

// Redact scans text and replaces discovered secrets with stable placeholders.
func Redact(text string) (string, *RedactionMap) {
	rm := NewRedactionMap()
	return rm.RedactString(text), rm
}

// RedactString mutates rm while redacting text (for multi-message pipelines).
func (rm *RedactionMap) RedactString(text string) string {
	result := text
	for _, pat := range sensitivePatterns {
		result = pat.ReplaceAllStringFunc(result, func(match string) string {
			rm.mu.Lock()
			defer rm.mu.Unlock()
			if ph, ok := rm.reverse[match]; ok {
				return ph
			}
			rm.seq++
			placeholder := "[REDACTED_SECRET_" + itoa(rm.seq) + "]"
			rm.forward[placeholder] = match
			rm.reverse[match] = placeholder
			return placeholder
		})
	}
	return result
}

// RedactRequest redacts all message text fields in-place on a cloned request.
func RedactRequest(req *models.ChatCompletionRequest) (*models.ChatCompletionRequest, *RedactionMap) {
	out := req.Clone()
	rm := NewRedactionMap()
	for i, msg := range out.Messages {
		text := rm.RedactString(msg.Content.Text())
		content, err := msg.Content.WithText(text)
		if err != nil {
			content, _ = models.FlexContent{}.WithText(text)
		}
		out.Messages[i].Content = content
	}
	return out, rm
}

// RedactAnthropicRequest redacts system and message text on a cloned Anthropic request.
func RedactAnthropicRequest(req *models.MessagesRequest) (*models.MessagesRequest, *RedactionMap) {
	out := req.Clone()
	rm := NewRedactionMap()

	if out.System.Text() != "" {
		text := rm.RedactString(out.System.Text())
		content, err := out.System.WithText(text)
		if err == nil {
			out.System = content
		}
	}

	for i, msg := range out.Messages {
		text := rm.RedactString(msg.Content.Text())
		content, err := msg.Content.WithText(text)
		if err != nil {
			content, _ = models.FlexContent{}.WithText(text)
		}
		out.Messages[i].Content = content
	}
	return out, rm
}

// Restore reverses placeholder masking on inbound streaming text.
func (rm *RedactionMap) Restore(text string) string {
	if rm == nil || len(rm.forward) == 0 {
		return text
	}
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	result := text
	for placeholder, original := range rm.forward {
		result = strings.ReplaceAll(result, placeholder, original)
	}
	return result
}

// Len returns the number of active redactions.
func (rm *RedactionMap) Len() int {
	if rm == nil {
		return 0
	}
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	return len(rm.forward)
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var buf [20]byte
	i := len(buf)
	for n > 0 {
		i--
		buf[i] = byte('0' + n%10)
		n /= 10
	}
	return string(buf[i:])
}
