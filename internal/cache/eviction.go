package cache

import (
	"context"
	"encoding/binary"
	"log/slog"
	"time"

	bolt "go.etcd.io/bbolt"
)

// StartEvictionWorker runs periodic sweeps until ctx is cancelled.
func (s *Store) StartEvictionWorker(ctx context.Context, interval time.Duration, logger *slog.Logger) {
	if s.ttl <= 0 || interval <= 0 {
		return
	}
	if logger == nil {
		logger = slog.Default()
	}

	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()

		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				n, err := s.sweepExpiredKeys()
				if err != nil {
					logger.Error("cache eviction sweep failed", "err", err)
					continue
				}
				if n > 0 {
					logger.Info("cache eviction sweep", "deleted", n)
				}
			}
		}
	}()
}

func (s *Store) sweepExpiredKeys() (int, error) {
	if s.ttl <= 0 {
		return 0, nil
	}

	now := time.Now().UnixNano()
	var deleted int

	err := s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucketName))
		if b == nil {
			return nil
		}

		c := b.Cursor()
		for k, v := c.First(); k != nil; k, v = c.Next() {
			if len(v) < expiryPrefixLen || v[0] == '{' {
				continue
			}
			expiresAt := int64(binary.BigEndian.Uint64(v[:expiryPrefixLen]))
			if expiresAt > 0 && now > expiresAt {
				if err := b.Delete(k); err != nil {
					return err
				}
				deleted++
			}
		}
		return nil
	})

	return deleted, err
}
