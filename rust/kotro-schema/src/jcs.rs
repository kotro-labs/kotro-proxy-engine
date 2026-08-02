//! RFC 8785 JSON Canonicalization Scheme helpers.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Canonicalize `value` with RFC 8785 JCS.
pub fn canonicalize(value: &Value) -> Result<Vec<u8>, String> {
    serde_json_canonicalizer::to_vec(value).map_err(|e| e.to_string())
}

/// `sha256(JCS(value))` as lowercase hex, prefixed with `sha256:`.
pub fn args_hash(value: &Value) -> Result<String, String> {
    let bytes = canonicalize(value)?;
    let digest = Sha256::digest(&bytes);
    Ok(format!("sha256:{digest:x}"))
}

/// Short 16-hex digest used by the existing approval API.
pub fn short_args_hash(value: &Value) -> Result<String, String> {
    let bytes = canonicalize(value)?;
    let digest = Sha256::digest(&bytes);
    Ok(format!("{digest:x}")[..16].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_hash() {
        let a = json!({"b": 1, "a": 2});
        let b = json!({"a": 2, "b": 1});
        assert_eq!(args_hash(&a).unwrap(), args_hash(&b).unwrap());
    }
}
