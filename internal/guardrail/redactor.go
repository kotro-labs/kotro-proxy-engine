// Package guardrail implements the local privacy guardrail (Feature B).
package guardrail

import (
	"strings"
	"sync"

	"github.com/kortolabs/proxy-engine/internal/models"
)

// RedactionMap holds placeholder -> original value mappings for a single request.
type RedactionMap struct {
	mu            sync.RWMutex
	forward       map[string]string
	reverse       map[string]string
	patternCounts map[string]int
	seq           int
}

// NewRedactionMap creates an empty per-request redaction registry.
func NewRedactionMap() *RedactionMap {
	return &RedactionMap{
		forward:       make(map[string]string),
		reverse:       make(map[string]string),
		patternCounts: make(map[string]int),
	}
}

// Redact scans text and replaces discovered secrets with stable placeholders.
func Redact(text string) (string, *RedactionMap) {
	rm := NewRedactionMap()
	return rm.RedactString(text), rm
}

// RedactString mutates rm while redacting text (for multi-message pipelines).
func (rm *RedactionMap) RedactString(text string) string {
	result := text
	for _, spec := range classifiedPatterns {
		result = spec.re.ReplaceAllStringFunc(result, func(match string) string {
			rm.mu.Lock()
			defer rm.mu.Unlock()
			if ph, ok := rm.reverse[match]; ok {
				return ph
			}
			rm.seq++
			placeholder := "[REDACTED_SECRET_" + itoa(rm.seq) + "]"
			rm.forward[placeholder] = match
			rm.reverse[match] = placeholder
			rm.patternCounts[spec.label]++
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
	out, _ := rm.RestoreCounted(text)
	return out
}

// RestoreCounted restores placeholders and returns the number of replacements.
func (rm *RedactionMap) RestoreCounted(text string) (string, int) {
	if rm == nil || len(rm.forward) == 0 {
		return text, 0
	}
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	result := text
	restores := 0
	for placeholder, original := range rm.forward {
		if strings.Contains(result, placeholder) {
			restores += strings.Count(result, placeholder)
			result = strings.ReplaceAll(result, placeholder, original)
		}
	}
	return result, restores
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
