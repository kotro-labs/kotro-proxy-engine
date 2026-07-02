package cache

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/kortolabs/proxy-engine/internal/metrics"
	bolt "go.etcd.io/bbolt"
)

const bucketName = "sse_cache"

// StoreOptions configures TTL and background eviction for the embedded cache.
type StoreOptions struct {
	TTL               time.Duration // 0 disables expiration
	EvictionInterval  time.Duration
	EnableCompression bool
}

// Store is a bbolt-backed embedded database for cached SSE streams.
type Store struct {
	db        *bolt.DB
	path      string
	ttl       time.Duration
	compress  bool
	metrics   *metrics.Registry
	mu        sync.RWMutex
}

// Open creates or opens the cache database at path with no TTL.
func Open(path string) (*Store, error) {
	return OpenWithOptions(path, StoreOptions{})
}

// OpenWithOptions creates or opens the cache database with TTL settings.
func OpenWithOptions(path string, opts StoreOptions) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil && filepath.Dir(path) != "." {
		return nil, fmt.Errorf("cache: mkdir: %w", err)
	}

	db, err := bolt.Open(path, 0o600, &bolt.Options{Timeout: 2 * time.Second})
	if err != nil {
		return nil, fmt.Errorf("cache: open db: %w", err)
	}

	if err := db.Update(func(tx *bolt.Tx) error {
		_, err := tx.CreateBucketIfNotExists([]byte(bucketName))
		return err
	}); err != nil {
		_ = db.Close()
		return nil, fmt.Errorf("cache: init bucket: %w", err)
	}

	return &Store{db: db, path: path, ttl: opts.TTL, compress: opts.EnableCompression}, nil
}

// Get retrieves a cached SSE stream by semantic key. Returns nil, nil on miss or expiry.
func (s *Store) Get(key string) (*Entry, error) {
	var raw []byte
	err := s.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucketName))
		if b == nil {
			return nil
		}
		raw = append([]byte(nil), b.Get([]byte(key))...)
		return nil
	})
	if err != nil {
		return nil, err
	}
	if raw == nil {
		return nil, nil
	}

	payload, expired := decodeStoredValue(raw, time.Now().UnixNano())
	if expired {
		go func(k string) { _ = s.Delete(k) }(key)
		return nil, nil
	}

	var entry Entry
	if err := json.Unmarshal(payload, &entry); err != nil {
		return nil, fmt.Errorf("cache: corrupt entry: %w", err)
	}
	return &entry, nil
}

// Put writes a complete SSE stream entry with an optional expiration prefix.
func (s *Store) Put(entry Entry) error {
	payload, err := json.Marshal(entry)
	if err != nil {
		return err
	}
	stored := encodeStoredValue(expiresAtNano(s.ttl), payload, s.compress)

	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucketName))
		return b.Put([]byte(entry.Key), stored)
	})
}

// Delete removes a cache entry by key.
func (s *Store) Delete(key string) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucketName))
		if b == nil {
			return nil
		}
		return b.Delete([]byte(key))
	})
}

// SetMetrics attaches a Prometheus registry for cache gauges and evictions.
func (s *Store) SetMetrics(m *metrics.Registry) {
	s.metrics = m
}

// Count returns the number of keys in the cache bucket.
func (s *Store) Count() (int, error) {
	var n int
	err := s.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucketName))
		if b == nil {
			return nil
		}
		stats := b.Stats()
		n = stats.KeyN
		return nil
	})
	return n, err
}

// TTL returns the configured entry lifetime (0 = no expiry).
func (s *Store) TTL() time.Duration { return s.ttl }

// SweepExpired deletes all keys whose TTL prefix has lapsed.
func (s *Store) SweepExpired() (int, error) {
	n, err := s.sweepExpiredKeys()
	if err == nil && s.metrics != nil {
		s.metrics.RecordCacheEvictions("ttl", n)
		if count, cerr := s.Count(); cerr == nil {
			s.metrics.SetCacheEntries(count)
		}
	}
	return n, err
}

// PutRaw stores a raw bucket value (used for legacy migration tests).
func (s *Store) PutRaw(key string, value []byte) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucketName))
		if b == nil {
			return fmt.Errorf("cache: bucket missing")
		}
		return b.Put([]byte(key), value)
	})
}

// Close shuts down the embedded database.
func (s *Store) Close() error {
	return s.db.Close()
}

// Path returns the on-disk database file path.
func (s *Store) Path() string { return s.path }
