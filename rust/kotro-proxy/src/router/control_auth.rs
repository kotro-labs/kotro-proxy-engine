//! Authentication for mutating control-plane endpoints (kill switch, approvals,
//! action-plane event ingestion).
//!
//! - A random per-install control token is generated and stored (0600) under
//!   the state dir, or supplied via `KOTRO_CONTROL_TOKEN`.
//! - Comparison is constant-time.
//! - Browser cross-origin requests are rejected: when an `Origin` header is
//!   present it must be a loopback origin (dashboard served from the proxy).

use std::path::Path;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use rand::RngCore;

pub const CONTROL_TOKEN_HEADER: &str = "x-kotro-control-token";
pub const CONTROL_TOKEN_FILE: &str = "control.token";

/// Load the control token from `<state_dir>/control.token`, creating it with
/// mode 0600 if missing.
pub fn load_or_create_control_token(state_dir: &Path) -> std::io::Result<String> {
    let path = state_dir.join(CONTROL_TOKEN_FILE);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let token = existing.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    std::fs::create_dir_all(state_dir)?;
    let token = generate_token();
    std::fs::write(&path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time byte comparison — no early exit on first mismatch.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn origin_is_local(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    // Authority only (drop path/query). Handle IPv6 bracket form `[::1]:port`.
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = if let Some(inside) = authority.strip_prefix('[') {
        inside.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn presented_token(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get(CONTROL_TOKEN_HEADER).and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Gate for mutating control endpoints. Returns `Err(response)` when the
/// request must be rejected.
pub fn require_control_token(expected: &str, headers: &HeaderMap) -> Result<(), Response> {
    // Cross-origin browser requests are always rejected (CSRF defense);
    // same-origin dashboard fetches send a loopback Origin or none at all.
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if !origin_is_local(origin) {
            return Err((
                StatusCode::FORBIDDEN,
                "control API rejects cross-origin requests",
            )
                .into_response());
        }
    }

    let Some(presented) = presented_token(headers) else {
        return Err((
            StatusCode::UNAUTHORIZED,
            "control token required (x-kotro-control-token header); \
             see <state_dir>/control.token",
        )
            .into_response());
    };
    if !constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        return Err((StatusCode::UNAUTHORIZED, "invalid control token").into_response());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn constant_time_eq_basic() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn token_file_roundtrip_and_perms() {
        let dir = tempfile::tempdir().unwrap();
        let t1 = load_or_create_control_token(dir.path()).unwrap();
        let t2 = load_or_create_control_token(dir.path()).unwrap();
        assert_eq!(t1, t2);
        assert_eq!(t1.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(CONTROL_TOKEN_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn rejects_missing_and_wrong_token() {
        let headers = HeaderMap::new();
        assert!(require_control_token("tok", &headers).is_err());

        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_TOKEN_HEADER, HeaderValue::from_static("wrong"));
        assert!(require_control_token("tok", &headers).is_err());
    }

    #[test]
    fn accepts_header_and_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_TOKEN_HEADER, HeaderValue::from_static("tok"));
        assert!(require_control_token("tok", &headers).is_ok());

        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer tok"));
        assert!(require_control_token("tok", &headers).is_ok());
    }

    #[test]
    fn rejects_foreign_origin_even_with_token() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_TOKEN_HEADER, HeaderValue::from_static("tok"));
        headers.insert("origin", HeaderValue::from_static("https://evil.example"));
        assert!(require_control_token("tok", &headers).is_err());

        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_TOKEN_HEADER, HeaderValue::from_static("tok"));
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:9090"));
        assert!(require_control_token("tok", &headers).is_ok());
    }

    #[test]
    fn accepts_ipv6_loopback_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_TOKEN_HEADER, HeaderValue::from_static("tok"));
        headers.insert("origin", HeaderValue::from_static("http://[::1]:9090"));
        assert!(require_control_token("tok", &headers).is_ok());
    }
}
