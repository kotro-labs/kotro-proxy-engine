package guardrail_test

import (
	"strings"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/guardrail"
	"github.com/kortolabs/proxy-engine/internal/models"
)

func TestRedactAWSKey(t *testing.T) {
	input := "Use key AKIAIOSFODNN7EXAMPLE for auth"
	out, rm := guardrail.Redact(input)
	if strings.Contains(out, "AKIA") {
		t.Fatalf("expected redacted output, got %q", out)
	}
	restored := rm.Restore(out)
	if !strings.Contains(restored, "AKIAIOSFODNN7EXAMPLE") {
		t.Fatalf("restore failed: %q", restored)
	}
}

func TestRedactEmail(t *testing.T) {
	input := "Contact me at dev@example.com please"
	out, rm := guardrail.Redact(input)
	if strings.Contains(out, "dev@example.com") {
		t.Fatalf("email not redacted: %q", out)
	}
	if !strings.Contains(rm.Restore(out), "dev@example.com") {
		t.Fatal("email restore failed")
	}
}

func TestRedactRequestMultimessage(t *testing.T) {
	req := &models.ChatCompletionRequest{
		Messages: []models.ChatMessage{
			{Role: "user", Content: mustFlex(`"key AKIAIOSFODNN7EXAMPLE"`)},
			{Role: "user", Content: mustFlex(`"email dev@example.com"`)},
		},
	}
	out, rm := guardrail.RedactRequest(req)
	body := out.Messages[0].Content.Text() + " " + out.Messages[1].Content.Text()
	if strings.Contains(body, "AKIA") || strings.Contains(body, "dev@example.com") {
		t.Fatalf("secrets not redacted: %q", body)
	}
	if rm.Len() != 2 {
		t.Fatalf("expected 2 redactions, got %d", rm.Len())
	}
}

func mustFlex(raw string) models.FlexContent {
	var f models.FlexContent
	if err := f.UnmarshalJSON([]byte(raw)); err != nil {
		panic(err)
	}
	return f
}
