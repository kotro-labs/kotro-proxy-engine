package models

import (
	"encoding/json"
	"fmt"
)

// MessagesRequest is the Anthropic /v1/messages inbound payload.
type MessagesRequest struct {
	Model     string           `json:"model"`
	System    FlexContent      `json:"system,omitempty"`
	Messages  []AnthropicTurn  `json:"messages"`
	Stream    bool             `json:"stream"`
	MaxTokens int              `json:"max_tokens,omitempty"`
}

// AnthropicTurn is one role/content pair in the messages array.
type AnthropicTurn struct {
	Role    string      `json:"role"`
	Content FlexContent `json:"content"`
}

// AnthropicDeltaEvent is a content_block_delta SSE data payload.
type AnthropicDeltaEvent struct {
	Type  string `json:"type"`
	Index int    `json:"index"`
	Delta struct {
		Type string `json:"type"`
		Text string `json:"text"`
	} `json:"delta"`
}

// ExtractPromptState returns system text and the latest user message for cache keying.
func (r *MessagesRequest) ExtractPromptState() (systemPrompt, latestUser string) {
	systemPrompt = r.System.Text()
	for _, msg := range r.Messages {
		if msg.Role == "user" {
			latestUser = msg.Content.Text()
		}
	}
	return systemPrompt, latestUser
}

// Clone returns a deep copy suitable for middleware mutation.
func (r *MessagesRequest) Clone() *MessagesRequest {
	out := *r
	out.Messages = make([]AnthropicTurn, len(r.Messages))
	copy(out.Messages, r.Messages)
	return &out
}

// Marshal serializes the request to JSON bytes.
func (r *MessagesRequest) Marshal() ([]byte, error) {
	return json.Marshal(r)
}

// ParseMessagesRequest decodes an Anthropic messages body.
func ParseMessagesRequest(body []byte) (*MessagesRequest, error) {
	var req MessagesRequest
	if err := json.Unmarshal(body, &req); err != nil {
		return nil, fmt.Errorf("parse messages request: %w", err)
	}
	return &req, nil
}
