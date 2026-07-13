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

#[derive(Debug, Clone, Default)]
pub struct ScopeResolver {
    pub trust_upstream_gateway: bool,
    pub trusted_proxy_cidrs: Vec<IpNet>,
}

impl ScopeResolver {
    pub fn from_request(&self, headers: &HeaderMap, peer: IpAddr) -> Scope {
        if self.trust_upstream_gateway && self.is_trusted_peer(peer) {
            return scope_from_trusted_headers(headers);
        }
        derive_scope_from_credentials(headers)
    }

    fn is_trusted_peer(&self, peer: IpAddr) -> bool {
        // Socket address from ConnectInfo only — never HTTP forwarding headers.
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

    // --- End-to-end tenant isolation: ScopeResolver -> Scope::key() ->
    // generate_cache_key(). The tests above prove ScopeResolver derives
    // different scopes for different credentials in isolation; the tests
    // below prove that difference actually survives the full chain into a
    // real cache key for the *same* request content, the way
    // router/handlers.rs::unified_cache_key wires it. Nothing else in the
    // suite exercises this chain together -- a regression that hard-coded
    // or dropped the scope on the way into generate_cache_key would not be
    // caught by the scope-only or cache-only unit tests on their own. This
    // is the Rust equivalent of Go's TestCacheIsolation_TenantSeparation /
    // TestAnthropicCacheIsolation_TenantSeparation (docs/roadmap/next-steps.md P1).

    fn bearer_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        headers
    }

    #[test]
    fn different_credentials_produce_different_cache_keys_for_identical_request() {
        let resolver = ScopeResolver::default();
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        let scope_a = resolver.from_request(&bearer_headers("user-a-secret-token"), peer);
        let scope_b = resolver.from_request(&bearer_headers("user-b-secret-token"), peer);

        // Different principals must resolve to different scopes...
        assert_ne!(scope_a.key(), scope_b.key());

        // ...and, critically, that difference must survive into the actual
        // cache key for byte-identical request material (same model, same
        // provider, same prompt) -- otherwise two agent sessions from
        // different users/keys could read each other's cached responses.
        let material = b"system:hi||user:what's in this repo?";
        let key_a = crate::cache::generate_cache_key(&scope_a.key(), "gpt-4", "openai", material);
        let key_b = crate::cache::generate_cache_key(&scope_b.key(), "gpt-4", "openai", material);
        assert_ne!(key_a, key_b, "different credentials must not share a cache entry");
    }

    #[test]
    fn same_credential_produces_the_same_cache_key_across_requests() {
        // Isolation without correctness is useless -- a scheme that made
        // every request its own tenant would "pass" the test above too.
        // Confirm the same principal replaying the same request still
        // lands on the same key, i.e. cache hits are still possible at all.
        let resolver = ScopeResolver::default();
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        let scope_1 = resolver.from_request(&bearer_headers("same-user-token"), peer);
        let scope_2 = resolver.from_request(&bearer_headers("same-user-token"), peer);
        assert_eq!(scope_1.key(), scope_2.key());

        let material = b"system:hi||user:what's in this repo?";
        let key_1 = crate::cache::generate_cache_key(&scope_1.key(), "gpt-4", "openai", material);
        let key_2 = crate::cache::generate_cache_key(&scope_2.key(), "gpt-4", "openai", material);
        assert_eq!(key_1, key_2);
    }

    #[test]
    fn missing_credentials_share_the_default_scope_not_a_forged_tenant() {
        // No Authorization/x-api-key header at all falls back to the shared
        // "default:default" scope (documented, intentional -- see
        // docs/security/THREAT-MODEL.md 4.1). Anonymous local mock traffic
        // shares a cache; anything with a credential gets its own
        // cred:<hash> partition regardless of what headers claim.
        let resolver = ScopeResolver::default();
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        let scope = resolver.from_request(&HeaderMap::new(), peer);
        assert_eq!(scope.tenant_id, "default");
        assert_eq!(scope.session_id, "default");
    }

    #[test]
    fn trusted_gateway_headers_isolate_tenants_through_the_same_chain() {
        // Same end-to-end proof as above, but for the gateway/multi-tenant
        // path (KOTRO_TRUST_UPSTREAM_GATEWAY=true) rather than the default
        // credential-derived path.
        let resolver = ScopeResolver {
            trust_upstream_gateway: true,
            trusted_proxy_cidrs: vec!["127.0.0.0/8".parse().unwrap()],
        };
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        let mut headers_a = HeaderMap::new();
        headers_a.insert(HEADER_TENANT_ID, HeaderValue::from_static("tenant-a"));
        let mut headers_b = HeaderMap::new();
        headers_b.insert(HEADER_TENANT_ID, HeaderValue::from_static("tenant-b"));

        let scope_a = resolver.from_request(&headers_a, peer);
        let scope_b = resolver.from_request(&headers_b, peer);

        let material = b"system:hi||user:what's in this repo?";
        let key_a = crate::cache::generate_cache_key(&scope_a.key(), "gpt-4", "openai", material);
        let key_b = crate::cache::generate_cache_key(&scope_b.key(), "gpt-4", "openai", material);
        assert_ne!(key_a, key_b, "gateway-assigned tenants must not share a cache entry");
    }
}
