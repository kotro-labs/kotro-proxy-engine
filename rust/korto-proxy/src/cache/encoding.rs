//! 8-byte big-endian expiration prefix — byte-identical to Go `internal/cache/encoding.go`.

pub const EXPIRY_PREFIX_LEN: usize = 8;

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

fn is_zstd_frame(payload: &[u8]) -> bool {
    payload.len() >= 4 && payload[..4] == ZSTD_MAGIC
}

/// Prepends an absolute expiration timestamp to the JSON payload.
/// When `enable_compression` is true, the payload is ZSTD-framed before the prefix.
/// `expires_at_nano == 0` stores without a prefix (no TTL).
pub fn encode_stored_value(
    expires_at_nano: i64,
    payload: &[u8],
    enable_compression: bool,
) -> Vec<u8> {
    if expires_at_nano <= 0 {
        return payload.to_vec();
    }

    let target_payload = if enable_compression && !payload.is_empty() {
        zstd::encode_all(payload, 3).unwrap_or_else(|_| payload.to_vec())
    } else {
        payload.to_vec()
    };

    let mut buf = Vec::with_capacity(EXPIRY_PREFIX_LEN + target_payload.len());
    buf.extend_from_slice(&(expires_at_nano as u64).to_be_bytes());
    buf.extend_from_slice(&target_payload);
    buf
}

/// Strips the expiration prefix, auto-detects ZSTD frames, and reports expiry.
/// Legacy entries beginning with `{` never expire (Go migration compat).
pub fn decode_stored_value(raw: &[u8], now_nano: i64) -> (Option<Vec<u8>>, bool) {
    if raw.is_empty() {
        return (None, true);
    }
    if raw[0] == b'{' {
        return (Some(raw.to_vec()), false);
    }
    if raw.len() < EXPIRY_PREFIX_LEN {
        return (None, true);
    }

    let expires_at =
        u64::from_be_bytes(raw[..EXPIRY_PREFIX_LEN].try_into().unwrap()) as i64;
    let payload = &raw[EXPIRY_PREFIX_LEN..];

    let payload = if is_zstd_frame(payload) {
        match zstd::decode_all(payload) {
            Ok(decompressed) => decompressed,
            Err(_) => payload.to_vec(),
        }
    } else {
        payload.to_vec()
    };

    if expires_at > 0 && now_nano > expires_at {
        return (None, true);
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
        let encoded = encode_stored_value(exp, payload, false);
        let (decoded, expired) = decode_stored_value(&encoded, exp - 1);
        assert!(!expired);
        assert_eq!(decoded.as_deref(), Some(payload.as_slice()));
    }

    #[test]
    fn roundtrip_compressed_entry() {
        let payload = br#"{"Key":"k","RawSSE":"data: x"}"#;
        let exp = 1_700_000_000_000_000_000i64;
        let encoded = encode_stored_value(exp, payload, true);
        assert!(is_zstd_frame(&encoded[EXPIRY_PREFIX_LEN..]));
        let (decoded, expired) = decode_stored_value(&encoded, exp - 1);
        assert!(!expired);
        assert_eq!(decoded.as_deref(), Some(payload.as_slice()));
    }

    #[test]
    fn detects_expired_entry() {
        let payload = br#"{"Key":"k"}"#;
        let exp = 1_000i64;
        let encoded = encode_stored_value(exp, payload, false);
        let (_, expired) = decode_stored_value(&encoded, exp + 1);
        assert!(expired);
    }

    #[test]
    fn legacy_json_without_prefix() {
        let legacy = br#"{"Key":"legacy"}"#;
        let (out, expired) = decode_stored_value(legacy, i64::MAX);
        assert!(!expired);
        assert_eq!(out.as_deref(), Some(legacy.as_slice()));
    }

    #[test]
    fn prefix_layout_matches_go() {
        let payload = br#"{"x":1}"#;
        let exp: u64 = 123_456_789;
        let encoded = encode_stored_value(exp as i64, payload, false);
        assert_eq!(encoded.len(), EXPIRY_PREFIX_LEN + payload.len());
        assert_eq!(u64::from_be_bytes(encoded[..8].try_into().unwrap()), exp);
        assert_eq!(&encoded[8..], payload);
    }
}
