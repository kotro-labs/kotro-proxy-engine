package compressor

import "fmt"

// Scope isolates compressor state to a tenant/session pair.
type Scope struct {
	TenantID  string
	SessionID string
}

func (s Scope) Key() string {
	return fmt.Sprintf("%s:%s", s.TenantID, s.SessionID)
}
