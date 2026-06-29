// Package models defines OpenAI-compatible request/response structs used by the
// proxy's streaming cache and middleware pipeline.
package models

import (
	"encoding/json"
	"fmt"
)

// ChatCompletionRequest is the inbound payload from local IDE agents.
type ChatCompletionRequest struct {
	Model       string        `json:"model"`
	Messages    []ChatMessage `json:"messages"`
	Stream      bool          `json:"stream"`
	Temperature *float64      `json:"temperature,omitempty"`
	MaxTokens   *int          `json:"max_tokens,omitempty"`
}

// ChatMessage represents a single turn in the conversation array.
// Content accepts both plain strings and multimodal part arrays from Cursor.
type ChatMessage struct {
	Role    string      `json:"role"`
	Content FlexContent `json:"content"`
}

// FlexContent holds string or multimodal array content from agent payloads.
type FlexContent struct {
	raw json.RawMessage
}

// UnmarshalJSON preserves the original JSON for lossless re-marshaling upstream.
func (f *FlexContent) UnmarshalJSON(b []byte) error {
	f.raw = append(json.RawMessage(nil), b...)
	return nil
}

// MarshalJSON emits the original content shape unchanged.
func (f FlexContent) MarshalJSON() ([]byte, error) {
	if len(f.raw) == 0 {
		return []byte(`""`), nil
	}
	return f.raw, nil
}

// Text extracts plain text for cache keying, redaction, and compression.
func (f FlexContent) Text() string {
	if len(f.raw) == 0 {
		return ""
	}
	if f.raw[0] == '"' {
		var s string
		if err := json.Unmarshal(f.raw, &s); err == nil {
			return s
		}
	}
	var parts []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if err := json.Unmarshal(f.raw, &parts); err == nil {
		var out string
		for _, p := range parts {
			if p.Type == "text" && p.Text != "" {
				out += p.Text
			}
		}
		return out
	}
	return string(f.raw)
}

// WithText returns a copy with text replaced, preserving string vs array shape.
func (f FlexContent) WithText(text string) (FlexContent, error) {
	if len(f.raw) == 0 || f.raw[0] == '"' {
		b, err := json.Marshal(text)
		return FlexContent{raw: b}, err
	}
	var parts []map[string]any
	if err := json.Unmarshal(f.raw, &parts); err != nil {
		return FlexContent{}, err
	}
	replaced := false
	for i, p := range parts {
		if t, _ := p["type"].(string); t == "text" {
			parts[i]["text"] = text
			replaced = true
		}
	}
	if !replaced {
		parts = append([]map[string]any{{"type": "text", "text": text}}, parts...)
	}
	b, err := json.Marshal(parts)
	return FlexContent{raw: b}, err
}

// StreamChunk is a single SSE data line from OpenAI-compatible providers.
type StreamChunk struct {
	ID      string         `json:"id,omitempty"`
	Object  string         `json:"object,omitempty"`
	Created int64          `json:"created,omitempty"`
	Model   string         `json:"model,omitempty"`
	Choices []StreamChoice `json:"choices"`
}

// StreamChoice holds the delta fragment for one streaming choice.
type StreamChoice struct {
	Index        int         `json:"index"`
	Delta        StreamDelta `json:"delta"`
	FinishReason *string     `json:"finish_reason"`
}

// StreamDelta is the incremental text payload inside a streaming choice.
type StreamDelta struct {
	Role    string `json:"role,omitempty"`
	Content string `json:"content,omitempty"`
}

// ExtractPromptState returns the system prompt text and the latest user message
// content — the two inputs used for semantic cache keying.
func (r *ChatCompletionRequest) ExtractPromptState() (systemPrompt, latestUser string) {
	for _, msg := range r.Messages {
		switch msg.Role {
		case "system":
			systemPrompt = msg.Content.Text()
		case "user":
			latestUser = msg.Content.Text()
		}
	}
	return systemPrompt, latestUser
}

// Clone returns a deep copy suitable for mutation by middleware.
func (r *ChatCompletionRequest) Clone() *ChatCompletionRequest {
	out := *r
	out.Messages = make([]ChatMessage, len(r.Messages))
	copy(out.Messages, r.Messages)
	return &out
}

// Marshal serializes the request to JSON bytes.
func (r *ChatCompletionRequest) Marshal() ([]byte, error) {
	return json.Marshal(r)
}

// ParseChatCompletionRequest decodes a JSON body into ChatCompletionRequest.
func ParseChatCompletionRequest(body []byte) (*ChatCompletionRequest, error) {
	var req ChatCompletionRequest
	if err := json.Unmarshal(body, &req); err != nil {
		return nil, fmt.Errorf("parse chat completion: %w", err)
	}
	return &req, nil
}
