package sse_test

import (
	"bytes"
	"io"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/sse"
)

func TestReaderPreservesFrames(t *testing.T) {
	raw := "data: {\"x\":1}\n\ndata: [DONE]\n\n"
	r := sse.NewReader(bytes.NewReader([]byte(raw)))

	f1, err := r.Next()
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(f1.DataPayload(), []byte(`"x":1`)) {
		t.Fatalf("unexpected payload: %s", f1.DataPayload())
	}

	f2, err := r.Next()
	if err != nil {
		t.Fatal(err)
	}
	if !f2.IsDone() {
		t.Fatal("expected DONE frame")
	}

	f3 := sse.Frame{Lines: [][]byte{[]byte("event: message_stop"), []byte(`data: {"type":"message_stop"}`)}}
	if !f3.IsAnthropicComplete() {
		t.Fatal("expected anthropic message_stop to complete stream")
	}

	_, err = r.Next()
	if err != io.EOF {
		t.Fatalf("expected EOF, got %v", err)
	}
}

func TestTransformDataLine(t *testing.T) {
	frame := sse.Frame{Lines: [][]byte{[]byte(`data: {"content":"secret"}`)}}
	out := sse.TransformDataLine(frame, func(p []byte) []byte {
		return []byte(`{"content":"[REDACTED]"}`)
	})
	if string(out.DataPayload()) != `{"content":"[REDACTED]"}` {
		t.Fatalf("transform failed: %s", out.DataPayload())
	}
}
