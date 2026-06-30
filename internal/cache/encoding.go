package cache

import (
	"encoding/binary"
	"time"

	"github.com/klauspost/compress/zstd"
)

const expiryPrefixLen = 8

var (
	zstdEncoder, _ = zstd.NewWriter(nil, zstd.WithEncoderLevel(zstd.SpeedDefault))
	zstdDecoder, _ = zstd.NewReader(nil)
)

func isZstdFrame(payload []byte) bool {
	return len(payload) >= 4 &&
		payload[0] == 0x28 &&
		payload[1] == 0xb5 &&
		payload[2] == 0x2f &&
		payload[3] == 0xfd
}

// encodeStoredValue prepends an absolute expiration timestamp to the JSON payload.
// When enableCompression is true, the payload is ZSTD-framed before the prefix.
// expiresAtNano of 0 stores the payload without a prefix (no TTL).
func encodeStoredValue(expiresAtNano int64, payload []byte, enableCompression bool) []byte {
	if expiresAtNano <= 0 {
		return payload
	}

	targetPayload := payload
	if enableCompression && len(payload) > 0 {
		targetPayload = zstdEncoder.EncodeAll(payload, make([]byte, 0, len(payload)/2))
	}

	buf := make([]byte, expiryPrefixLen+len(targetPayload))
	binary.BigEndian.PutUint64(buf[:expiryPrefixLen], uint64(expiresAtNano))
	copy(buf[expiryPrefixLen:], targetPayload)
	return buf
}

// decodeStoredValue strips the expiration prefix, auto-detects ZSTD frames, and
// reports whether the entry lapsed. Legacy entries beginning with '{' never expire.
func decodeStoredValue(raw []byte, nowNano int64) (payload []byte, expired bool) {
	if len(raw) == 0 {
		return nil, true
	}
	if raw[0] == '{' {
		return raw, false
	}
	if len(raw) < expiryPrefixLen {
		return nil, true
	}

	expiresAt := int64(binary.BigEndian.Uint64(raw[:expiryPrefixLen]))
	payload = raw[expiryPrefixLen:]

	if isZstdFrame(payload) {
		if decompressed, err := zstdDecoder.DecodeAll(payload, nil); err == nil {
			payload = decompressed
		}
	}

	if expiresAt > 0 && nowNano > expiresAt {
		return nil, true
	}
	return payload, false
}

func expiresAtNano(ttl time.Duration) int64 {
	if ttl <= 0 {
		return 0
	}
	return time.Now().Add(ttl).UnixNano()
}
