package cache

import (
	"encoding/binary"
	"testing"
	"time"
)

func TestEncodeDecodeStoredValue(t *testing.T) {
	payload := []byte(`{"key":"k","raw_sse":"data: x"}`)
	exp := time.Now().Add(time.Hour).UnixNano()
	encoded := encodeStoredValue(exp, payload)

	decoded, expired := decodeStoredValue(encoded, time.Now().UnixNano())
	if expired {
		t.Fatal("expected active entry")
	}
	if string(decoded) != string(payload) {
		t.Fatalf("payload mismatch: %q", decoded)
	}

	_, expired = decodeStoredValue(encoded, exp+1)
	if !expired {
		t.Fatal("expected expired entry")
	}
}

func TestDecodeLegacyJSON(t *testing.T) {
	legacy := []byte(`{"Key":"k"}`)
	out, expired := decodeStoredValue(legacy, time.Now().UnixNano())
	if expired || string(out) != string(legacy) {
		t.Fatalf("legacy decode failed: %q expired=%v", out, expired)
	}
}

func TestExpiresAtNanoZeroWithoutTTL(t *testing.T) {
	if expiresAtNano(0) != 0 {
		t.Fatal("zero ttl should not set expiry")
	}
	if got := expiresAtNano(time.Hour); got <= 0 {
		t.Fatal("expected positive expiry timestamp")
	}
}

func TestExpiryPrefixLayout(t *testing.T) {
	payload := []byte(`{"x":1}`)
	exp := int64(123456789)
	encoded := encodeStoredValue(exp, payload)
	if len(encoded) != 8+len(payload) {
		t.Fatalf("unexpected encoded length %d", len(encoded))
	}
	if got := int64(binary.BigEndian.Uint64(encoded[:8])); got != exp {
		t.Fatalf("prefix timestamp %d want %d", got, exp)
	}
}
