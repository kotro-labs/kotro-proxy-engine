package optimizer_test

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/models"
	"github.com/kortolabs/proxy-engine/internal/optimizer"
)

func TestEnforceCacheMatrix_sortsContextFiles(t *testing.T) {
	req := &models.ChatCompletionRequest{
		Model: "gpt-4o",
		Messages: []models.ChatMessage{
			{Role: "system", Content: mustFlexText("sys")},
			{Role: "user", Content: mustFlexText("<file name=\"z.go\">z</file>")},
			{Role: "user", Content: mustFlexText("<file name=\"a.go\">a</file>")},
			{Role: "user", Content: mustFlexText("latest question")},
		},
	}

	optimizer.EnforceCacheMatrix(req)

	if len(req.Messages) != 4 {
		t.Fatalf("expected 4 messages, got %d", len(req.Messages))
	}
	if req.Messages[1].Content.Text() != "<file name=\"a.go\">a</file>" {
		t.Fatalf("expected sorted context first, got %q", req.Messages[1].Content.Text())
	}
	if req.Messages[2].Content.Text() != "<file name=\"z.go\">z</file>" {
		t.Fatalf("expected sorted context second, got %q", req.Messages[2].Content.Text())
	}
	if req.Messages[3].Content.Text() != "latest question" {
		t.Fatalf("expected latest user last, got %q", req.Messages[3].Content.Text())
	}
}

func TestEnforceCacheMatrix_keepsLongUserMessageInHistory(t *testing.T) {
	longUser := strings.Repeat("user asks about architecture ", 40)
	req := &models.ChatCompletionRequest{
		Model: "gpt-4o",
		Messages: []models.ChatMessage{
			{Role: "system", Content: mustFlexText("sys")},
			{Role: "user", Content: mustFlexText(longUser)},
		},
	}

	optimizer.EnforceCacheMatrix(req)

	if req.Messages[1].Content.Text() != longUser {
		t.Fatal("long user message should remain in place, not be treated as context dump")
	}
}

func TestEnforceCacheMatrix_preservesToolTurns(t *testing.T) {
	req := &models.ChatCompletionRequest{
		Model: "gpt-4o",
		Messages: []models.ChatMessage{
			{Role: "system", Content: mustFlexText("sys")},
			{Role: "user", Content: mustFlexText("start")},
			{Role: "tool", Content: mustFlexText("tool output"), ToolCallID: "call_1"},
			{Role: "user", Content: mustFlexText("continue")},
		},
	}

	optimizer.EnforceCacheMatrix(req)

	if req.Messages[2].Role != "tool" {
		t.Fatalf("expected tool turn preserved, got role %q", req.Messages[2].Role)
	}
}

func mustFlexText(s string) models.FlexContent {
	b, err := json.Marshal(s)
	if err != nil {
		panic(err)
	}
	var f models.FlexContent
	if err := json.Unmarshal(b, &f); err != nil {
		panic(err)
	}
	return f
}
