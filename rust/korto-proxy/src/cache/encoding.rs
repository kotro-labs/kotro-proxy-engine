//! 8-byte big-endian expiration prefix — byte-identical to Go `internal/cache/encoding.go`.

pub const EXPIRY_PREFIX_LEN: usize = 8;

/// Prepends an absolute expiration timestamp to the JSON payload.
/// `expires_at_nano == 0` stores without a prefix (no TTL).
pub fn encode_stored_value(expires_at_nano: i64, payload: &[u8]) -> Vec<u8> {
    if expires_at_nano <= 0 {
        return payload.to_vec();
    }
    let mut buf = Vec::with_capacity(EXPIRY_PREFIX_LEN + payload.len());
    buf.extend_from_slice(&(expires_at_nano as u64).to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Strips the expiration prefix and reports whether the entry lapsed.
/// Legacy entries beginning with `{` never expire (Go migration compat).
pub fn decode_stored_value(raw: &[u8], now_nano: i64) -> (Option<&[u8]>, bool) {
    if raw.is_empty() {
        return (None, true);
    }
    if raw[0] == b'{' {
        return (Some(raw), false);
    }
    if raw.len() < EXPIRY_PREFIX_LEN + 1 || raw[EXPIRY_PREFIX_LEN] != b'{' {
        return (None, true);
    }
    let expires_at =
        u64::from_be_bytes(raw[..EXPIRY_PREFIX_LEN].try_into().unwrap()) as i64;
    let payload = &raw[EXPIRY_PREFIX_LEN..];
    if expires_at > 0 && now_nano > expires_at {
        return (Some(payload), true);
    }
    (Some(payload), false)
}

pub fn expiry_prefix_len() -> usize {
    EXPIRY_PREFIX_LEN
}

pub fn expires_at_nano(ttl: std::time::Duration) -> i64 {
    if ttl.is_zero() {
        return 0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before epoch");
    (now + ttl).as_nanos() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_active_entry() {
        let payload = br#"{"Key":"k","RawSSE":"data: x"}"#;
        let exp = 1_700_000_000_000_000_000i64;
        let encoded = encode_stored_value(exp, payload);
        let (decoded, expired) = decode_stored_value(&encoded, exp - 1);
        assert!(!expired);
        assert_eq!(decoded, Some(payload.as_slice()));
    }

    #[test]
    fn detects_expired_entry() {
        let payload = br#"{"Key":"k"}"#;
        let exp = 1_000i64;
        let encoded = encode_stored_value(exp, payload);
        let (_, expired) = decode_stored_value(&encoded, exp + 1);
        assert!(expired);
    }

    #[test]
    fn legacy_json_without_prefix() {
        let legacy = br#"{"Key":"legacy"}"#;
        let (out, expired) = decode_stored_value(legacy, i64::MAX);
        assert!(!expired);
        assert_eq!(out, Some(legacy.as_slice()));
    }

    #[test]
    fn prefix_layout_matches_go() {
        let payload = br#"{"x":1}"#;
        let exp: u64 = 123_456_789;
        let encoded = encode_stored_value(exp as i64, payload);
        assert_eq!(encoded.len(), EXPIRY_PREFIX_LEN + payload.len());
        assert_eq!(u64::from_be_bytes(encoded[..8].try_into().unwrap()), exp);
        assert_eq!(&encoded[8..], payload);
    }
}
