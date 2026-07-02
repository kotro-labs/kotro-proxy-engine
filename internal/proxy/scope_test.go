package proxy

import (
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestScopeFromRequestUsesHeadersWhenTrusted(t *testing.T) {
	req := httptest.NewRequest("POST", "/v1/chat/completions", nil)
	req.Header.Set("X-Tenant-ID", "acme")
	req.Header.Set("X-Session-ID", "sess-42")
	req.RemoteAddr = "127.0.0.1:12345"

	resolver := ScopeResolver{
		TrustUpstreamGateway: true,
		TrustedProxyCIDRs:    mustParseCIDRs(t, "127.0.0.0/8"),
	}

	scope := resolver.FromRequest(req)
	if scope.TenantID != "acme" || scope.SessionID != "sess-42" {
		t.Fatalf("unexpected scope: %+v", scope)
	}
}

func TestScopeFromRequestIgnoresSpoofedHeaders(t *testing.T) {
	req := httptest.NewRequest("POST", "/v1/chat/completions", nil)
	req.Header.Set("X-Tenant-ID", "target-enterprise")
	req.Header.Set("Authorization", "Bearer secret-token")

	scope := ScopeResolver{}.FromRequest(req)
	if scope.TenantID == "target-enterprise" {
		t.Fatal("untrusted client must not control tenant scope via header")
	}
	if !strings.HasPrefix(scope.TenantID, "cred:") {
		t.Fatalf("expected credential-derived tenant, got %q", scope.TenantID)
	}
}

func TestScopeFromRequestHashesBearerToken(t *testing.T) {
	req := httptest.NewRequest("POST", "/v1/chat/completions", nil)
	req.Header.Set("Authorization", "Bearer secret-token")

	resolver := ScopeResolver{}
	scopeA := resolver.FromRequest(req)
	scopeB := resolver.FromRequest(req)
	if scopeA != scopeB {
		t.Fatal("same bearer token should map to stable scope")
	}

	req.Header.Set("Authorization", "Bearer other-token")
	scopeC := resolver.FromRequest(req)
	if scopeC == scopeA {
		t.Fatal("different bearer tokens must not share scope")
	}
}

func TestReadLimitedBodyRejectsOversizedPayload(t *testing.T) {
	body := strings.Repeat("x", 32)
	req := httptest.NewRequest("POST", "/v1/chat/completions", strings.NewReader(body))
	rec := httptest.NewRecorder()

	_, err := readLimitedBody(rec, req, 16)
	if err == nil {
		t.Fatal("expected oversize body to fail")
	}
	if rec.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("expected 413, got %d", rec.Code)
	}
}

func mustParseCIDRs(t *testing.T, raw string) []*net.IPNet {
	t.Helper()
	cidrs, err := parseTrustedCIDRs(raw)
	if err != nil {
		t.Fatal(err)
	}
	return cidrs
}
