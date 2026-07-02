package metrics

import (
	"time"

	dto "github.com/prometheus/client_model/go"
)

const (
	recentRequestCapacity = 10
	cacheWindowDuration   = 5 * time.Minute
)

// RecentRequest is a privacy-safe summary of a proxied call.
type RecentRequest struct {
	At          time.Time `json:"at"`
	Provider    string    `json:"provider"`
	Route       string    `json:"route"`
	CacheStatus string    `json:"cache_status"`
}

// DashboardSnapshot is the JSON payload for GET /api/dashboard.
type DashboardSnapshot struct {
	UpdatedAt              time.Time       `json:"updated_at"`
	CacheHitRate5m         float64         `json:"cache_hit_rate_5m"`
	CacheHits5m            int             `json:"cache_hits_5m"`
	CacheMisses5m          int             `json:"cache_misses_5m"`
	CompressorBytesSaved   float64         `json:"compressor_bytes_saved_total"`
	CompressorBlocksStripped float64       `json:"compressor_blocks_stripped_total"`
	CompressorScopesActive float64         `json:"compressor_scopes_active"`
	RedactionsTotal        float64         `json:"redactions_total"`
	CacheEntries           float64         `json:"cache_entries"`
	RequestsTotal          float64         `json:"requests_total"`
	RecentRequests         []RecentRequest `json:"recent_requests"`
}

type cacheWindowEvent struct {
	hit bool
	at  time.Time
}

func (r *Registry) noteDashboardRequest(provider, route, cacheStatus string) {
	r.dashboardMu.Lock()
	defer r.dashboardMu.Unlock()

	if cacheStatus == "hit" {
		r.cacheWindow = append(r.cacheWindow, cacheWindowEvent{hit: true, at: time.Now()})
	} else if cacheStatus == "miss" {
		r.cacheWindow = append(r.cacheWindow, cacheWindowEvent{hit: false, at: time.Now()})
	}
	r.pruneCacheWindowLocked()

	r.recentRequests = append([]RecentRequest{{
		At:          time.Now().UTC(),
		Provider:    provider,
		Route:       route,
		CacheStatus: cacheStatus,
	}}, r.recentRequests...)
	if len(r.recentRequests) > recentRequestCapacity {
		r.recentRequests = r.recentRequests[:recentRequestCapacity]
	}
}

func (r *Registry) pruneCacheWindowLocked() {
	cutoff := time.Now().Add(-cacheWindowDuration)
	i := 0
	for _, ev := range r.cacheWindow {
		if ev.at.After(cutoff) {
			r.cacheWindow[i] = ev
			i++
		}
	}
	r.cacheWindow = r.cacheWindow[:i]
}

func (r *Registry) cacheHitRate5mLocked() (rate float64, hits, misses int) {
	r.pruneCacheWindowLocked()
	for _, ev := range r.cacheWindow {
		if ev.hit {
			hits++
		} else {
			misses++
		}
	}
	total := hits + misses
	if total == 0 {
		return 0, hits, misses
	}
	return float64(hits) / float64(total), hits, misses
}

// Snapshot returns a developer-dashboard view of current metrics.
func (r *Registry) Snapshot() DashboardSnapshot {
	if r == nil {
		return DashboardSnapshot{UpdatedAt: time.Now().UTC()}
	}

	r.dashboardMu.Lock()
	rate, hits, misses := r.cacheHitRate5mLocked()
	recent := make([]RecentRequest, len(r.recentRequests))
	copy(recent, r.recentRequests)
	r.dashboardMu.Unlock()

	totals := r.gatherTotals()

	return DashboardSnapshot{
		UpdatedAt:                time.Now().UTC(),
		CacheHitRate5m:           rate,
		CacheHits5m:              hits,
		CacheMisses5m:            misses,
		CompressorBytesSaved:     totals["korto_compressor_bytes_saved_total"],
		CompressorBlocksStripped: totals["korto_compressor_blocks_stripped_total"],
		CompressorScopesActive:   totals["korto_compressor_scopes_active"],
		RedactionsTotal:          totals["korto_redactions_total"],
		CacheEntries:             totals["korto_cache_entries"],
		RequestsTotal:            totals["korto_requests_total"],
		RecentRequests:           recent,
	}
}

func (r *Registry) gatherTotals() map[string]float64 {
	out := map[string]float64{
		"korto_compressor_bytes_saved_total":     0,
		"korto_compressor_blocks_stripped_total": 0,
		"korto_compressor_scopes_active":       0,
		"korto_redactions_total":               0,
		"korto_cache_entries":                  0,
		"korto_requests_total":                 0,
	}
	mfs, err := r.prom.Gather()
	if err != nil {
		return out
	}
	for _, mf := range mfs {
		name := mf.GetName()
		switch name {
		case "korto_compressor_bytes_saved_total",
			"korto_compressor_blocks_stripped_total",
			"korto_compressor_scopes_active",
			"korto_cache_entries":
			out[name] = sumMetricFamily(mf)
		case "korto_redactions_total", "korto_requests_total":
			out[name] = sumMetricFamily(mf)
		}
	}
	return out
}

func sumMetricFamily(mf *dto.MetricFamily) float64 {
	var total float64
	for _, m := range mf.GetMetric() {
		switch {
		case m.Counter != nil:
			total += m.GetCounter().GetValue()
		case m.Gauge != nil:
			total += m.GetGauge().GetValue()
		}
	}
	return total
}
