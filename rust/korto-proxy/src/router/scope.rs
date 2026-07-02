//! Tenant/session scope extraction — mirrors `internal/proxy/scope.go`.

use std::net::IpAddr;

use axum::http::HeaderMap;
use ipnet::IpNet;
use sha2::{Digest, Sha256};

use crate::compressor::Scope;

const HEADER_TENANT_ID: &str = "x-tenant-id";
const HEADER_SESSION_ID: &str = "x-session-id";
const DEFAULT_TENANT_ID: &str = "default";
const DEFAULT_SESSION_ID: &str = "default";

#[derive(Debug, Clone)]
pub struct ScopeResolver {
    pub trust_upstream_gateway: bool,
    pub trusted_proxy_cidrs: Vec<IpNet>,
}

impl Default for ScopeResolver {
    fn default() -> Self {
        Self {
            trust_upstream_gateway: false,
            trusted_proxy_cidrs: Vec::new(),
        }
    }
}

impl ScopeResolver {
    pub fn from_request(&self, headers: &HeaderMap, peer: IpAddr) -> Scope {
        if self.trust_upstream_gateway && self.is_trusted_peer(peer) {
            return scope_from_trusted_headers(headers);
        }
        derive_scope_from_credentials(headers)
    }

    fn is_trusted_peer(&self, peer: IpAddr) -> bool {
        self.trusted_proxy_cidrs
            .iter()
            .any(|cidr| cidr.contains(&peer))
    }
}

fn scope_from_trusted_headers(headers: &HeaderMap) -> Scope {
    let tenant_id = headers
        .get(HEADER_TENANT_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let Some(tenant_id) = tenant_id else {
        return derive_scope_from_credentials(headers);
    };

    let session_id = headers
        .get(HEADER_SESSION_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| session_from_credentials(headers));

    Scope {
        tenant_id: tenant_id.to_string(),
        session_id,
    }
}

fn derive_scope_from_credentials(headers: &HeaderMap) -> Scope {
    let Some(cred) = extract_credential(headers) else {
        return Scope {
            tenant_id: DEFAULT_TENANT_ID.into(),
            session_id: DEFAULT_SESSION_ID.into(),
        };
    };

    let h = hash_credential(&cred);
    let scope_id = format!("cred:{h}");
    Scope {
        tenant_id: scope_id.clone(),
        session_id: scope_id,
    }
}

fn extract_credential(headers: &HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn session_from_credentials(headers: &HeaderMap) -> String {
    extract_credential(headers)
        .map(|cred| hash_credential(&cred))
        .unwrap_or_else(|| DEFAULT_SESSION_ID.to_string())
}

fn hash_credential(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn parse_trusted_cidrs(raw: &str) -> Result<Vec<IpNet>, String> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        out.push(
            part.parse::<IpNet>()
                .map_err(|err| format!("invalid CIDR {part}: {err}"))?,
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use std::net::Ipv4Addr;

    #[test]
    fn uses_headers_when_trusted() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_TENANT_ID, HeaderValue::from_static("acme"));
        headers.insert(HEADER_SESSION_ID, HeaderValue::from_static("sess-42"));

        let resolver = ScopeResolver {
            trust_upstream_gateway: true,
            trusted_proxy_cidrs: vec!["127.0.0.0/8".parse().unwrap()],
        };

        let scope = resolver.from_request(&headers, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(scope.tenant_id, "acme");
        assert_eq!(scope.session_id, "sess-42");
    }

    #[test]
    fn ignores_spoofed_headers_by_default() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_TENANT_ID, HeaderValue::from_static("target-enterprise"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer secret-token"),
        );

        let scope = ScopeResolver::default().from_request(&headers, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(scope.tenant_id, "target-enterprise");
        assert!(scope.tenant_id.starts_with("cred:"));
    }
}
