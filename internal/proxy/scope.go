package proxy

import (
	"crypto/sha256"
	"encoding/hex"
	"net/http"
	"strings"

	"github.com/kortolabs/proxy-engine/internal/compressor"
)

const (
	headerTenantID   = "X-Tenant-ID"
	headerSessionID  = "X-Session-ID"
	defaultTenantID  = "default"
	defaultSessionID = "default"
)

func scopeFromRequest(r *http.Request) compressor.Scope {
	tenant := strings.TrimSpace(r.Header.Get(headerTenantID))
	if tenant == "" {
		tenant = defaultTenantID
	}

	session := strings.TrimSpace(r.Header.Get(headerSessionID))
	if session == "" {
		session = sessionFromCredentials(r)
	}

	return compressor.Scope{TenantID: tenant, SessionID: session}
}

func sessionFromCredentials(r *http.Request) string {
	if auth := r.Header.Get("Authorization"); strings.HasPrefix(auth, "Bearer ") {
		token := strings.TrimSpace(strings.TrimPrefix(auth, "Bearer "))
		if token != "" {
			return hashCredential(token)
		}
	}
	if apiKey := strings.TrimSpace(r.Header.Get("x-api-key")); apiKey != "" {
		return hashCredential(apiKey)
	}
	return defaultSessionID
}

func hashCredential(value string) string {
	sum := sha256.Sum256([]byte(value))
	return hex.EncodeToString(sum[:8])
}
