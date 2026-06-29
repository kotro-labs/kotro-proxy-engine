package cache

import (
	"encoding/binary"
	"time"
)

const expiryPrefixLen = 8

// encodeStoredValue prepends an absolute expiration timestamp to the JSON payload.
// expiresAtNano of 0 stores the payload without a prefix (no TTL).
func encodeStoredValue(expiresAtNano int64, payload []byte) []byte {
	if expiresAtNano <= 0 {
		return payload
	}
	buf := make([]byte, expiryPrefixLen+len(payload))
	binary.BigEndian.PutUint64(buf[:expiryPrefixLen], uint64(expiresAtNano))
	copy(buf[expiryPrefixLen:], payload)
	return buf
}

// decodeStoredValue strips the expiration prefix and reports whether the entry lapsed.
// Legacy entries written before TTL support begin with '{' and never expire.
func decodeStoredValue(raw []byte, nowNano int64) (payload []byte, expired bool) {
	if len(raw) == 0 {
		return nil, true
	}
	if raw[0] == '{' {
		return raw, false
	}
	if len(raw) < expiryPrefixLen+1 || raw[expiryPrefixLen] != '{' {
		return nil, true
	}
	expiresAt := int64(binary.BigEndian.Uint64(raw[:expiryPrefixLen]))
	if expiresAt > 0 && nowNano > expiresAt {
		return raw[expiryPrefixLen:], true
	}
	return raw[expiryPrefixLen:], false
}

func expiresAtNano(ttl time.Duration) int64 {
	if ttl <= 0 {
		return 0
	}
	return time.Now().Add(ttl).UnixNano()
}
