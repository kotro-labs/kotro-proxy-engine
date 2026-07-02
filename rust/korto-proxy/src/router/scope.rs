//! Tenant/session scope extraction — mirrors `internal/proxy/scope.go`.

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

use crate::compressor::Scope;

const HEADER_TENANT_ID: &str = "x-tenant-id";
const HEADER_SESSION_ID: &str = "x-session-id";
const DEFAULT_TENANT_ID: &str = "default";
const DEFAULT_SESSION_ID: &str = "default";

pub fn scope_from_headers(headers: &HeaderMap) -> Scope {
    let tenant_id = headers
        .get(HEADER_TENANT_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_TENANT_ID)
        .to_string();

    let session_id = headers
        .get(HEADER_SESSION_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| session_from_credentials(headers));

    Scope {
        tenant_id,
        session_id,
    }
}

fn session_from_credentials(headers: &HeaderMap) -> String {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return hash_credential(token);
            }
        }
    }
    if let Some(api_key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        let api_key = api_key.trim();
        if !api_key.is_empty() {
            return hash_credential(api_key);
        }
    }
    DEFAULT_SESSION_ID.to_string()
}

fn hash_credential(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn uses_explicit_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_TENANT_ID, HeaderValue::from_static("acme"));
        headers.insert(HEADER_SESSION_ID, HeaderValue::from_static("sess-42"));

        let scope = scope_from_headers(&headers);
        assert_eq!(scope.tenant_id, "acme");
        assert_eq!(scope.session_id, "sess-42");
    }

    #[test]
    fn hashes_bearer_token_into_session_scope() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer secret-token"),
        );

        let scope = scope_from_headers(&headers);
        assert_ne!(scope.session_id, DEFAULT_SESSION_ID);
    }
}
