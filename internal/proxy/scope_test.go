package proxy

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestScopeFromRequestUsesHeaders(t *testing.T) {
	req := httptest.NewRequest("POST", "/v1/chat/completions", nil)
	req.Header.Set("X-Tenant-ID", "acme")
	req.Header.Set("X-Session-ID", "sess-42")

	scope := scopeFromRequest(req)
	if scope.TenantID != "acme" || scope.SessionID != "sess-42" {
		t.Fatalf("unexpected scope: %+v", scope)
	}
}

func TestScopeFromRequestHashesBearerToken(t *testing.T) {
	req := httptest.NewRequest("POST", "/v1/chat/completions", nil)
	req.Header.Set("Authorization", "Bearer secret-token")

	scopeA := scopeFromRequest(req)
	scopeB := scopeFromRequest(req)
	if scopeA.SessionID != scopeB.SessionID {
		t.Fatal("same bearer token should map to stable session id")
	}
	if scopeA.SessionID == defaultSessionID {
		t.Fatal("expected hashed session id for bearer token")
	}

	req.Header.Set("Authorization", "Bearer other-token")
	scopeC := scopeFromRequest(req)
	if scopeC.SessionID == scopeA.SessionID {
		t.Fatal("different bearer tokens must not share session scope")
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
