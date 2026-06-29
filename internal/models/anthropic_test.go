package models_test

import (
	"testing"

	"github.com/kortolabs/proxy-engine/internal/models"
)

func TestAnthropicExtractPromptState(t *testing.T) {
	req := &models.MessagesRequest{
		System: mustFlex(`"system rules"`),
		Messages: []models.AnthropicTurn{
			{Role: "user", Content: mustFlex(`"first"`)},
			{Role: "assistant", Content: mustFlex(`"mid"`)},
			{Role: "user", Content: mustFlex(`"latest"`)},
		},
	}
	sys, user := req.ExtractPromptState()
	if sys != "system rules" || user != "latest" {
		t.Fatalf("sys=%q user=%q", sys, user)
	}
}

func TestParseMessagesRequest(t *testing.T) {
	body := []byte(`{"model":"claude-3-5-sonnet-20241022","stream":true,"max_tokens":128,"messages":[{"role":"user","content":"hello"}]}`)
	req, err := models.ParseMessagesRequest(body)
	if err != nil {
		t.Fatal(err)
	}
	if !req.Stream || req.Model == "" {
		t.Fatalf("unexpected parse: %+v", req)
	}
}
