//! HTTP/2 SSE bootstrap comment — mirrors `internal/proxy/sse_bootstrap.go`.

use bytes::Bytes;

pub const BOOTSTRAP_COMMENT: &str = ": kortolabs bootstrap stream\n\n";

pub fn bootstrap_bytes() -> Bytes {
    Bytes::from_static(BOOTSTRAP_COMMENT.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_is_valid_sse() {
        assert!(BOOTSTRAP_COMMENT.starts_with(": "));
        assert!(BOOTSTRAP_COMMENT.ends_with("\n\n"));
    }
}
