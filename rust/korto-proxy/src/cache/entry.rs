//! Cache entry schema — mirrors `internal/cache/semantic.go` (`Entry`).

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

/// Complete concatenated SSE stream captured on cache miss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "RawSSE", with = "base64_bytes")]
    pub raw_sse: Vec<u8>,
    #[serde(rename = "Model")]
    pub model: String,
    #[serde(rename = "CreatedAt", default)]
    pub created_at: i64,
}

/// Go `encoding/json` marshals `[]byte` as a base64 string.
mod base64_bytes {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        STANDARD.encode(bytes).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        STANDARD
            .decode(encoded)
            .map_err(serde::de::Error::custom)
    }
}
