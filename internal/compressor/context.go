// Package compressor implements local context & MCP deduplication (Feature C).
package compressor

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"sync"

	"github.com/kortolabs/proxy-engine/internal/models"
)

// StateTracker remembers prior-turn context blocks per tenant/session scope to
// strip unchanged MCP schemas, directory trees, and other repeated blocks.
type StateTracker struct {
	mu     sync.Mutex
	scopes map[string]map[string]string // scope key -> block hash -> content
}

// NewStateTracker creates an empty per-process context diff tracker.
func NewStateTracker() *StateTracker {
	return &StateTracker{
		scopes: make(map[string]map[string]string),
	}
}

func blockHash(content string) string {
	h := sha256.Sum256([]byte(content))
	return hex.EncodeToString(h[:8])
}

// SplitBlocks divides message content into logical blocks separated by blank lines.
func SplitBlocks(content string) []string {
	parts := strings.Split(content, "\n\n")
	var blocks []string
	for _, p := range parts {
		if trimmed := strings.TrimSpace(p); trimmed != "" {
			blocks = append(blocks, trimmed)
		}
	}
	if len(blocks) == 0 && content != "" {
		return []string{content}
	}
	return blocks
}

// CompressMessage removes blocks identical to the previous turn for the scope.
func (st *StateTracker) CompressMessage(scope Scope, content string) (string, bool) {
	blocks := SplitBlocks(content)
	if len(blocks) == 0 {
		return content, false
	}

	st.mu.Lock()
	defer st.mu.Unlock()

	scopeKey := scope.Key()
	lastBlocks := st.scopes[scopeKey]
	if lastBlocks == nil {
		lastBlocks = make(map[string]string)
	}

	var kept []string
	changed := false
	current := make(map[string]string, len(blocks))

	for _, block := range blocks {
		hash := blockHash(block)
		current[hash] = block
		if prev, ok := lastBlocks[hash]; ok && prev == block {
			changed = true
			continue
		}
		kept = append(kept, block)
	}

	st.scopes[scopeKey] = current
	if !changed {
		return content, false
	}
	if len(kept) == 0 {
		return "", true
	}
	return strings.Join(kept, "\n\n"), true
}

// CompressRequest prunes redundant system/user message blocks across turns.
func (st *StateTracker) CompressRequest(scope Scope, req *models.ChatCompletionRequest) *models.ChatCompletionRequest {
	out := req.Clone()
	for i, msg := range out.Messages {
		if msg.Role != "system" && msg.Role != "user" {
			continue
		}
		text := msg.Content.Text()
		if pruned, ok := st.CompressMessage(scope, text); ok {
			content, err := msg.Content.WithText(pruned)
			if err == nil {
				out.Messages[i].Content = content
			}
		}
	}
	return out
}

// CompressAnthropicRequest prunes redundant system and user blocks across turns.
func (st *StateTracker) CompressAnthropicRequest(scope Scope, req *models.MessagesRequest) *models.MessagesRequest {
	out := req.Clone()

	if out.System.Text() != "" {
		if pruned, ok := st.CompressMessage(scope, out.System.Text()); ok {
			if content, err := out.System.WithText(pruned); err == nil {
				out.System = content
			}
		}
	}

	for i, msg := range out.Messages {
		if msg.Role != "user" {
			continue
		}
		text := msg.Content.Text()
		if pruned, ok := st.CompressMessage(scope, text); ok {
			content, err := msg.Content.WithText(pruned)
			if err == nil {
				out.Messages[i].Content = content
			}
		}
	}
	return out
}
