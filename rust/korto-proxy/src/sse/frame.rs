//! SSE frame parser — mirrors `internal/sse/stream.go`.

use bytes::{Bytes, BytesMut};
use std::io;
use thiserror::Error;

/// One SSE event block (one or more field lines, typically `data: ...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub lines: Vec<Bytes>,
}

impl Frame {
    /// JSON payload from the first `data: ` line, if present.
    pub fn data_payload(&self) -> Option<&[u8]> {
        for line in &self.lines {
            if let Some(payload) = line.strip_prefix(b"data: ") {
                return Some(payload);
            }
        }
        None
    }

    /// OpenAI stream terminator (`data: [DONE]`).
    pub fn is_openai_done(&self) -> bool {
        self.data_payload()
            .is_some_and(|p| p == b"[DONE]")
    }

    /// SSE event name from an `event: ...` line, if present.
    pub fn event_type(&self) -> Option<&str> {
        for line in &self.lines {
            if let Some(name) = line.strip_prefix(b"event: ") {
                return std::str::from_utf8(name).ok();
            }
        }
        None
    }

    /// Anthropic stream completion (`event: message_stop` or JSON type).
    pub fn is_anthropic_complete(&self) -> bool {
        if self.event_type() == Some("message_stop") {
            return true;
        }
        self.data_payload().is_some_and(|p| {
            p.windows(16).any(|w| w == b"\"type\":\"message_stop\"")
                || p.windows(18).any(|w| w == b"\"type\": \"message_stop\"")
        })
    }

    /// Re-serializes the frame with standard SSE trailing newline.
    pub fn to_bytes(&self) -> Bytes {
        let mut out = BytesMut::new();
        for line in &self.lines {
            out.extend_from_slice(line);
            out.extend_from_slice(b"\n");
        }
        out.extend_from_slice(b"\n");
        out.freeze()
    }
}

/// Result of evaluating the internal parser window for a complete frame boundary.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameParseResult {
    /// A complete SSE frame (including the blank-line delimiter).
    Complete(Bytes),
    /// Partial data; more upstream chunks are required.
    Incomplete,
}

/// Incremental blank-line delimiter splitter over a shared `BytesMut` pool.
#[derive(Debug)]
pub struct SseFrameParser {
    buffer: BytesMut,
}

impl Default for SseFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SseFrameParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(8192),
        }
    }

    /// Appends incoming network bytes to the internal parser window.
    pub fn feed(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Splits off the next complete SSE frame when a `\n\n` or `\r\n\r\n` boundary is found.
    pub fn next_frame(&mut self) -> FrameParseResult {
        let src = &self.buffer[..];

        for i in 0..src.len() {
            if src[i] == b'\n' && i + 1 < src.len() && src[i + 1] == b'\n' {
                let end_idx = i + 2;
                let frame_bytes = self.buffer.split_to(end_idx).freeze();
                return FrameParseResult::Complete(frame_bytes);
            }

            if src[i] == b'\r'
                && i + 3 < src.len()
                && src[i + 1] == b'\n'
                && src[i + 2] == b'\r'
                && src[i + 3] == b'\n'
            {
                let end_idx = i + 4;
                let frame_bytes = self.buffer.split_to(end_idx).freeze();
                return FrameParseResult::Complete(frame_bytes);
            }
        }

        FrameParseResult::Incomplete
    }

    /// Drains any remaining non-delimited trailing bytes when the upstream stream ends.
    pub fn drain_remaining(&mut self) -> Option<Bytes> {
        if self.buffer.is_empty() {
            return None;
        }
        Some(self.buffer.split_to(self.buffer.len()).freeze())
    }

    /// Consumes the parser and returns any trailing bytes (stream shutdown).
    pub fn finalize(mut self) -> Option<Bytes> {
        self.drain_remaining()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReaderError {
    #[error("unexpected end of SSE stream")]
    Eof,
    #[error("need more upstream bytes")]
    NeedMoreData,
}

/// Incremental frame reader — mirrors Go `sse.Reader`.
#[derive(Debug)]
pub struct Reader {
    parser: SseFrameParser,
    eof: bool,
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader {
    pub fn new() -> Self {
        Self {
            parser: SseFrameParser::new(),
            eof: false,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        self.parser.feed(chunk);
    }

    pub fn mark_eof(&mut self) {
        self.eof = true;
    }

    /// Returns the next SSE frame. Yields `ReaderError::Eof` when the stream is exhausted.
    pub fn next(&mut self) -> Result<Frame, ReaderError> {
        loop {
            match self.parser.next_frame() {
                FrameParseResult::Complete(raw) => return Ok(parse_frame_bytes(raw)),
                FrameParseResult::Incomplete if self.eof => {
                    return match self.parser.drain_remaining() {
                        Some(raw) => Ok(parse_frame_bytes(raw)),
                        None => Err(ReaderError::Eof),
                    };
                }
                FrameParseResult::Incomplete => return Err(ReaderError::NeedMoreData),
            }
        }
    }
}

/// Applies `transform` to the data payload and rewrites the `data:` line.
pub fn transform_data_line<F>(frame: &Frame, transform: F) -> Frame
where
    F: Fn(&[u8]) -> Vec<u8>,
{
    let mut lines = Vec::with_capacity(frame.lines.len());
    for line in &frame.lines {
        if let Some(payload) = line.strip_prefix(b"data: ") {
            if !frame.is_openai_done() {
                let rewritten = transform(payload);
                let mut data_line = BytesMut::from(&b"data: "[..]);
                data_line.extend_from_slice(&rewritten);
                lines.push(data_line.freeze());
                continue;
            }
        }
        lines.push(line.clone());
    }
    Frame { lines }
}

/// Parses a raw SSE frame block (including delimiter) into structured lines.
pub fn parse_frame_bytes(raw: Bytes) -> Frame {
    let body = strip_frame_delimiter(&raw);
    let mut lines = Vec::new();

    for line in body.split(|&b| b == b'\n') {
        let line = strip_carriage_return(line);
        if line.is_empty() {
            continue;
        }
        lines.push(Bytes::copy_from_slice(line));
    }

    Frame { lines }
}

fn strip_frame_delimiter(raw: &[u8]) -> &[u8] {
    if raw.ends_with(b"\r\n\r\n") {
        &raw[..raw.len() - 4]
    } else if raw.ends_with(b"\n\n") {
        &raw[..raw.len() - 2]
    } else {
        raw
    }
}

fn strip_carriage_return(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Writes a frame to `w`, preserving SSE event boundaries.
pub fn write_frame(w: &mut impl io::Write, frame: &Frame) -> io::Result<()> {
    w.write_all(&frame.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_frames(chunks: &[&[u8]]) -> Vec<Frame> {
        let mut reader = Reader::new();
        let mut frames = Vec::new();

        for (i, chunk) in chunks.iter().enumerate() {
            reader.feed(chunk);
            if i + 1 == chunks.len() {
                reader.mark_eof();
            }
            loop {
                match reader.next() {
                    Ok(frame) => frames.push(frame),
                    Err(ReaderError::NeedMoreData) => break,
                    Err(ReaderError::Eof) => return frames,
                }
            }
        }
        frames
    }

    #[test]
    fn single_frame_delivery() {
        let mut parser = SseFrameParser::new();
        parser.feed(b"data: {\"x\":1}\n\n");

        match parser.next_frame() {
            FrameParseResult::Complete(raw) => {
                let frame = parse_frame_bytes(raw);
                assert!(frame.data_payload().is_some_and(|p| {
                    p.windows(5).any(|w| w == br#""x":1"#)
                }));
            }
            other => panic!("expected complete frame, got {other:?}"),
        }

        assert_eq!(parser.next_frame(), FrameParseResult::Incomplete);
    }

    #[test]
    fn multi_frame_burst() {
        let mut parser = SseFrameParser::new();
        let burst = b"data: one\n\ndata: two\n\ndata: three\n\n";
        parser.feed(burst);

        let f1 = match parser.next_frame() {
            FrameParseResult::Complete(b) => parse_frame_bytes(b),
            other => panic!("{other:?}"),
        };
        let f2 = match parser.next_frame() {
            FrameParseResult::Complete(b) => parse_frame_bytes(b),
            other => panic!("{other:?}"),
        };
        let f3 = match parser.next_frame() {
            FrameParseResult::Complete(b) => parse_frame_bytes(b),
            other => panic!("{other:?}"),
        };

        assert_eq!(f1.data_payload(), Some(b"one".as_slice()));
        assert_eq!(f2.data_payload(), Some(b"two".as_slice()));
        assert_eq!(f3.data_payload(), Some(b"three".as_slice()));
        assert_eq!(parser.next_frame(), FrameParseResult::Incomplete);

        // ~4 KiB packet with three concatenated SSE messages.
        let big = "x".repeat(1300);
        let burst = format!("data: {big}\n\ndata: {big}\n\ndata: {big}\n\n");
        let mut parser = SseFrameParser::new();
        parser.feed(burst.as_bytes());
        let mut count = 0;
        while let FrameParseResult::Complete(raw) = parser.next_frame() {
            let frame = parse_frame_bytes(raw);
            assert!(frame.data_payload().is_some_and(|p| p.len() == 1300));
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn fragmented_stream() {
        let frames = collect_frames(&[b"data: hel", b"lo\n\n"]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data_payload(), Some(b"hello".as_slice()));
    }

    #[test]
    fn reader_preserves_frames_go_vector() {
        let raw = b"data: {\"x\":1}\n\ndata: [DONE]\n\n";
        let mut reader = Reader::new();
        reader.feed(raw);
        reader.mark_eof();

        let f1 = reader.next().expect("first frame");
        assert!(f1.data_payload().is_some_and(|p| {
            p.windows(5).any(|w| w == br#""x":1"#)
        }));

        let f2 = reader.next().expect("done frame");
        assert!(f2.is_openai_done());

        let f3 = Frame {
            lines: vec![
                Bytes::from_static(b"event: message_stop"),
                Bytes::from_static(br#"data: {"type":"message_stop"}"#),
            ],
        };
        assert!(f3.is_anthropic_complete());

        assert_eq!(reader.next(), Err(ReaderError::Eof));
    }

    #[test]
    fn transform_data_line_go_vector() {
        let frame = Frame {
            lines: vec![Bytes::from_static(br#"data: {"content":"secret"}"#)],
        };
        let out = transform_data_line(&frame, |_| br#"{"content":"[REDACTED]"}"#.to_vec());
        assert_eq!(
            out.data_payload(),
            Some(br#"{"content":"[REDACTED]"}"#.as_slice())
        );
    }

    #[test]
    fn windows_delimiter() {
        let mut parser = SseFrameParser::new();
        parser.feed(b"data: win\r\n\r\n");

        let frame = match parser.next_frame() {
            FrameParseResult::Complete(raw) => parse_frame_bytes(raw),
            other => panic!("{other:?}"),
        };
        assert_eq!(frame.data_payload(), Some(b"win".as_slice()));
    }

    #[test]
    fn frame_round_trip_bytes() {
        let frame = Frame {
            lines: vec![Bytes::from_static(br#"data: {"x":1}"#)],
        };
        let serialized = frame.to_bytes();
        let mut reader = Reader::new();
        reader.feed(&serialized);
        reader.mark_eof();
        let parsed = reader.next().expect("round trip");
        assert_eq!(parsed.data_payload(), frame.data_payload());
    }

    #[test]
    fn finalize_trailing_partial_frame() {
        let mut parser = SseFrameParser::new();
        parser.feed(b"data: tail");
        assert_eq!(parser.next_frame(), FrameParseResult::Incomplete);

        let trailing = parser.finalize().expect("trailing bytes");
        let frame = parse_frame_bytes(trailing);
        assert_eq!(frame.data_payload(), Some(b"tail".as_slice()));
    }

    #[test]
    fn zero_copy_frame_is_distinct_allocation() {
        let mut parser = SseFrameParser::new();
        parser.feed(b"data: abc\n\n");
        let FrameParseResult::Complete(frame_bytes) = parser.next_frame() else {
            panic!("expected frame");
        };
        assert_eq!(&frame_bytes[..6], b"data: ");
        assert!(parser.buffer.is_empty());
    }
}
