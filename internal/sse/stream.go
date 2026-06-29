// Package sse provides low-allocation Server-Sent Events framing for the proxy's
// streaming cache interceptor. OpenAI-compatible providers emit "data: {json}\n\n"
// frames; this package preserves blank-line event boundaries on replay.
package sse

import (
	"bufio"
	"bytes"
	"io"
)

// Frame is one SSE event block (one or more field lines, typically "data: ...").
type Frame struct {
	Lines [][]byte
}

// DataPayload returns the JSON payload from the first "data: " line, or nil.
func (f Frame) DataPayload() []byte {
	for _, line := range f.Lines {
		if len(line) >= 6 && string(line[:6]) == "data: " {
			return line[6:]
		}
	}
	return nil
}

// IsDone reports whether this frame is the OpenAI stream terminator.
func (f Frame) IsDone() bool {
	p := f.DataPayload()
	return p != nil && string(p) == "[DONE]"
}

// EventType returns the SSE event name from an "event: ..." line, if present.
func (f Frame) EventType() string {
	for _, line := range f.Lines {
		if len(line) >= 7 && string(line[:7]) == "event: " {
			return string(line[7:])
		}
	}
	return ""
}

// IsAnthropicComplete reports whether this frame ends an Anthropic stream.
func (f Frame) IsAnthropicComplete() bool {
	if f.EventType() == "message_stop" {
		return true
	}
	p := f.DataPayload()
	return p != nil && containsJSONType(p, "message_stop")
}

func containsJSONType(payload []byte, typ string) bool {
	needle := []byte(`"type":"` + typ + `"`)
	needleSpaced := []byte(`"type": "` + typ + `"`)
	return bytes.Contains(payload, needle) || bytes.Contains(payload, needleSpaced)
}

// Bytes re-serializes the frame with standard SSE trailing newline.
func (f Frame) Bytes() []byte {
	var out []byte
	for _, line := range f.Lines {
		out = append(out, line...)
		out = append(out, '\n')
	}
	out = append(out, '\n')
	return out
}

// Reader incrementally parses SSE frames from an upstream body.
type Reader struct {
	scanner *bufio.Scanner
	pending []byte
	err     error
}

// NewReader wraps r with a large-line buffer for big JSON chunks.
func NewReader(r io.Reader) *Reader {
	s := bufio.NewScanner(r)
	s.Buffer(make([]byte, 64*1024), 2*1024*1024)
	return &Reader{scanner: s}
}

// Next returns the next SSE frame. io.EOF when the stream ends cleanly.
func (r *Reader) Next() (Frame, error) {
	if r.err != nil {
		return Frame{}, r.err
	}

	var lines [][]byte
	for {
		if r.pending != nil {
			line := r.pending
			r.pending = nil
			if len(line) == 0 {
				if len(lines) > 0 {
					return Frame{Lines: lines}, nil
				}
				continue
			}
			lines = append(lines, line)
			continue
		}

		if !r.scanner.Scan() {
			if err := r.scanner.Err(); err != nil {
				r.err = err
				return Frame{}, err
			}
			if len(lines) > 0 {
				return Frame{Lines: lines}, nil
			}
			return Frame{}, io.EOF
		}

		line := append([]byte(nil), r.scanner.Bytes()...)
		if len(line) == 0 {
			if len(lines) > 0 {
				return Frame{Lines: lines}, nil
			}
			continue
		}
		lines = append(lines, line)
	}
}

// WriteFrame writes a frame to w, preserving SSE event boundaries.
func WriteFrame(w io.Writer, frame Frame) error {
	_, err := w.Write(frame.Bytes())
	return err
}

// TransformDataLine applies fn to the data payload and rewrites the data line.
func TransformDataLine(frame Frame, fn func(payload []byte) []byte) Frame {
	out := Frame{Lines: make([][]byte, len(frame.Lines))}
	for i, line := range frame.Lines {
		if len(line) >= 6 && string(line[:6]) == "data: " {
			payload := line[6:]
			if fn != nil && !frame.IsDone() {
				transformed := fn(payload)
				out.Lines[i] = append([]byte("data: "), transformed...)
				continue
			}
		}
		out.Lines[i] = append([]byte(nil), line...)
	}
	return out
}
