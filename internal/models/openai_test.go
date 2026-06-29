package models_test

import (
	"testing"

	"github.com/kortolabs/proxy-engine/internal/models"
)

func TestFlexContentString(t *testing.T) {
	body := []byte(`{"model":"m","messages":[{"role":"user","content":"hello"}]}`)
	req, err := models.ParseChatCompletionRequest(body)
	if err != nil {
		t.Fatal(err)
	}
	if got := req.Messages[0].Content.Text(); got != "hello" {
		t.Fatalf("got %q", got)
	}
}

func TestFlexContentArray(t *testing.T) {
	body := []byte(`{"model":"m","messages":[{"role":"user","content":[{"type":"text","text":"hello world"}]}]}`)
	req, err := models.ParseChatCompletionRequest(body)
	if err != nil {
		t.Fatal(err)
	}
	if got := req.Messages[0].Content.Text(); got != "hello world" {
		t.Fatalf("got %q", got)
	}
}

func TestExtractPromptState(t *testing.T) {
	req := &models.ChatCompletionRequest{
		Messages: []models.ChatMessage{
			{Role: "system", Content: mustFlex(`"sys prompt"`)},
			{Role: "user", Content: mustFlex(`"first"`)},
			{Role: "assistant", Content: mustFlex(`"mid"`)},
			{Role: "user", Content: mustFlex(`"latest"`)},
		},
	}
	sys, user := req.ExtractPromptState()
	if sys != "sys prompt" || user != "latest" {
		t.Fatalf("sys=%q user=%q", sys, user)
	}
}

func mustFlex(raw string) models.FlexContent {
	var f models.FlexContent
	_ = f.UnmarshalJSON([]byte(raw))
	return f
}
