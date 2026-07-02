// Package metrics exposes Prometheus instrumentation for the Kotro proxy.
package metrics

import (
	"net/http"
	"runtime"
	"strconv"
	"sync"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	collectors "github.com/prometheus/client_golang/prometheus/collectors"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

var (
	durationBuckets = []float64{0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10}
	bodyBuckets     = []float64{1024, 4096, 16384, 65536, 262144, 1048576, 5242880, 10485760}
)

// Registry holds Kotro Prometheus collectors.
type Registry struct {
	prom                  *prometheus.Registry
	requestsTotal         *prometheus.CounterVec
	requestDuration       *prometheus.HistogramVec
	upstreamDuration      *prometheus.HistogramVec
	requestBodyBytes      *prometheus.HistogramVec
	errorsTotal           *prometheus.CounterVec
	cacheHitsTotal        *prometheus.CounterVec
	cacheMissesTotal      *prometheus.CounterVec
	cacheStoresTotal      *prometheus.CounterVec
	cacheReplayBytesTotal *prometheus.CounterVec
	cacheEntries          prometheus.Gauge
	cacheEvictionsTotal   *prometheus.CounterVec
	compressorBlocks      prometheus.Counter
	compressorBytesSaved  prometheus.Counter
	compressorScopes      prometheus.Gauge
	compressorEvictions   *prometheus.CounterVec
	redactionsTotal       *prometheus.CounterVec
	redactionRestores     prometheus.Counter
	scopeModeTotal        *prometheus.CounterVec
	trustedPeerRejections prometheus.Counter
	goroutines            prometheus.Gauge
	residentMemoryBytes   prometheus.Gauge

	stopOnce sync.Once
	stopCh   chan struct{}

	dashboardMu    sync.Mutex
	recentRequests []RecentRequest
	cacheWindow    []cacheWindowEvent
}

// NewRegistry registers all Kotro metrics on an isolated Prometheus registry.
func NewRegistry() *Registry {
	prom := prometheus.NewRegistry()
	r := &Registry{
		prom: prom,
		requestsTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_requests_total",
			Help: "Total intercepted proxy requests.",
		}, []string{"provider", "route", "stream"}),
		requestDuration: prometheus.NewHistogramVec(prometheus.HistogramOpts{
			Name:    "korto_request_duration_seconds",
			Help:    "End-to-end handler latency in seconds.",
			Buckets: durationBuckets,
		}, []string{"provider", "cache_status"}),
		upstreamDuration: prometheus.NewHistogramVec(prometheus.HistogramOpts{
			Name:    "korto_upstream_duration_seconds",
			Help:    "Upstream round-trip latency on cache miss paths.",
			Buckets: durationBuckets,
		}, []string{"provider", "status_class"}),
		requestBodyBytes: prometheus.NewHistogramVec(prometheus.HistogramOpts{
			Name:    "korto_request_body_bytes",
			Help:    "Incoming JSON request body size in bytes.",
			Buckets: bodyBuckets,
		}, []string{"provider"}),
		errorsTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_errors_total",
			Help: "Proxy errors grouped by class.",
		}, []string{"provider", "error_class"}),
		cacheHitsTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_cache_hits_total",
			Help: "Semantic cache hits.",
		}, []string{"provider"}),
		cacheMissesTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_cache_misses_total",
			Help: "Semantic cache misses.",
		}, []string{"provider"}),
		cacheStoresTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_cache_stores_total",
			Help: "New cache entries written after complete streams.",
		}, []string{"provider"}),
		cacheReplayBytesTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_cache_replay_bytes_total",
			Help: "Bytes served from cache on hit replays.",
		}, []string{"provider"}),
		cacheEntries: prometheus.NewGauge(prometheus.GaugeOpts{
			Name: "korto_cache_entries",
			Help: "Approximate number of live cache entries.",
		}),
		cacheEvictionsTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_cache_evictions_total",
			Help: "Cache entry evictions.",
		}, []string{"reason"}),
		compressorBlocks: prometheus.NewCounter(prometheus.CounterOpts{
			Name: "korto_compressor_blocks_stripped_total",
			Help: "Context blocks removed as duplicates.",
		}),
		compressorBytesSaved: prometheus.NewCounter(prometheus.CounterOpts{
			Name: "korto_compressor_bytes_saved_total",
			Help: "Estimated bytes not sent upstream after compression.",
		}),
		compressorScopes: prometheus.NewGauge(prometheus.GaugeOpts{
			Name: "korto_compressor_scopes_active",
			Help: "Active compressor scope entries.",
		}),
		compressorEvictions: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_compressor_scope_evictions_total",
			Help: "Compressor scope evictions.",
		}, []string{"reason"}),
		redactionsTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_redactions_total",
			Help: "Secrets redacted before upstream.",
		}, []string{"pattern"}),
		redactionRestores: prometheus.NewCounter(prometheus.CounterOpts{
			Name: "korto_redaction_restores_total",
			Help: "Placeholder restores in streaming responses.",
		}),
		scopeModeTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name: "korto_scope_mode_total",
			Help: "Request scope resolution mode.",
		}, []string{"mode"}),
		trustedPeerRejections: prometheus.NewCounter(prometheus.CounterOpts{
			Name: "korto_trusted_peer_rejections_total",
			Help: "Gateway scope headers ignored from untrusted peers.",
		}),
		goroutines: prometheus.NewGauge(prometheus.GaugeOpts{
			Name: "korto_goroutines",
			Help: "Current goroutine count.",
		}),
		residentMemoryBytes: prometheus.NewGauge(prometheus.GaugeOpts{
			Name: "korto_process_resident_memory_bytes",
			Help: "Process resident memory size in bytes.",
		}),
		stopCh: make(chan struct{}),
	}

	metricCollectors := []prometheus.Collector{
		r.requestsTotal,
		r.requestDuration,
		r.upstreamDuration,
		r.requestBodyBytes,
		r.errorsTotal,
		r.cacheHitsTotal,
		r.cacheMissesTotal,
		r.cacheStoresTotal,
		r.cacheReplayBytesTotal,
		r.cacheEntries,
		r.cacheEvictionsTotal,
		r.compressorBlocks,
		r.compressorBytesSaved,
		r.compressorScopes,
		r.compressorEvictions,
		r.redactionsTotal,
		r.redactionRestores,
		r.scopeModeTotal,
		r.trustedPeerRejections,
		r.goroutines,
		r.residentMemoryBytes,
	}
	for _, c := range metricCollectors {
		prom.MustRegister(c)
	}
	prom.MustRegister(collectors.NewGoCollector())
	prom.MustRegister(collectors.NewProcessCollector(collectors.ProcessCollectorOpts{}))
	return r
}

// Handler serves GET /metrics.
func (r *Registry) Handler() http.Handler {
	if r == nil {
		return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.WriteHeader(http.StatusNotFound)
		})
	}
	return promhttp.HandlerFor(r.prom, promhttp.HandlerOpts{})
}

// StartRuntimeCollector samples runtime gauges until Stop is called.
func (r *Registry) StartRuntimeCollector(interval time.Duration) {
	if r == nil {
		return
	}
	if interval <= 0 {
		interval = 15 * time.Second
	}
	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for {
			select {
			case <-r.stopCh:
				return
			case <-ticker.C:
				r.sampleRuntime()
			}
		}
	}()
}

// Stop ends the runtime collector goroutine.
func (r *Registry) Stop() {
	if r == nil {
		return
	}
	r.stopOnce.Do(func() { close(r.stopCh) })
}

func (r *Registry) sampleRuntime() {
	r.goroutines.Set(float64(runtime.NumGoroutine()))
	var ms runtime.MemStats
	runtime.ReadMemStats(&ms)
	r.residentMemoryBytes.Set(float64(ms.Sys))
}

// RecordRequest records request-plane metrics.
func (r *Registry) RecordRequest(provider, route, stream, cacheStatus string, elapsed time.Duration) {
	if r == nil {
		return
	}
	r.requestsTotal.WithLabelValues(provider, route, stream).Inc()
	r.requestDuration.WithLabelValues(provider, cacheStatus).Observe(elapsed.Seconds())
	r.noteDashboardRequest(provider, route, cacheStatus)
}

// RecordRequestBody observes incoming payload size.
func (r *Registry) RecordRequestBody(provider string, nbytes int) {
	if r == nil || nbytes < 0 {
		return
	}
	r.requestBodyBytes.WithLabelValues(provider).Observe(float64(nbytes))
}

// RecordUpstream observes upstream round-trip latency.
func (r *Registry) RecordUpstream(provider string, statusCode int, elapsed time.Duration) {
	if r == nil {
		return
	}
	r.upstreamDuration.WithLabelValues(provider, StatusClass(statusCode)).Observe(elapsed.Seconds())
}

// RecordError increments an error counter.
func (r *Registry) RecordError(provider, class string) {
	if r == nil {
		return
	}
	r.errorsTotal.WithLabelValues(provider, class).Inc()
}

// RecordCacheHit increments cache hit and replay byte counters.
func (r *Registry) RecordCacheHit(provider string, replayBytes int) {
	if r == nil {
		return
	}
	r.cacheHitsTotal.WithLabelValues(provider).Inc()
	if replayBytes > 0 {
		r.cacheReplayBytesTotal.WithLabelValues(provider).Add(float64(replayBytes))
	}
}

// RecordCacheMiss increments cache miss counter.
func (r *Registry) RecordCacheMiss(provider string) {
	if r == nil {
		return
	}
	r.cacheMissesTotal.WithLabelValues(provider).Inc()
}

// RecordCacheStore increments cache store counter.
func (r *Registry) RecordCacheStore(provider string) {
	if r == nil {
		return
	}
	r.cacheStoresTotal.WithLabelValues(provider).Inc()
}

// SetCacheEntries updates the live cache entry gauge.
func (r *Registry) SetCacheEntries(n int) {
	if r == nil {
		return
	}
	r.cacheEntries.Set(float64(n))
}

// RecordCacheEvictions increments TTL/manual eviction counters.
func (r *Registry) RecordCacheEvictions(reason string, n int) {
	if r == nil || n <= 0 {
		return
	}
	r.cacheEvictionsTotal.WithLabelValues(reason).Add(float64(n))
}

// RecordCompression records compressor savings.
func (r *Registry) RecordCompression(blocksStripped, bytesSaved int) {
	if r == nil {
		return
	}
	if blocksStripped > 0 {
		r.compressorBlocks.Add(float64(blocksStripped))
	}
	if bytesSaved > 0 {
		r.compressorBytesSaved.Add(float64(bytesSaved))
	}
}

// SetCompressorScopes updates active scope gauge.
func (r *Registry) SetCompressorScopes(n int) {
	if r == nil {
		return
	}
	r.compressorScopes.Set(float64(n))
}

// RecordCompressorEviction records LRU/TTL scope eviction.
func (r *Registry) RecordCompressorEviction(reason string) {
	if r == nil {
		return
	}
	r.compressorEvictions.WithLabelValues(reason).Add(1)
}

// RecordRedaction increments a coarse pattern bucket.
func (r *Registry) RecordRedaction(pattern string) {
	if r == nil {
		return
	}
	r.redactionsTotal.WithLabelValues(pattern).Inc()
}

// RecordRedactionRestores increments stream restore operations.
func (r *Registry) RecordRedactionRestores(n int) {
	if r == nil || n <= 0 {
		return
	}
	r.redactionRestores.Add(float64(n))
}

// RecordScopeMode increments scope resolution mode counter.
func (r *Registry) RecordScopeMode(mode string) {
	if r == nil || mode == "" {
		return
	}
	r.scopeModeTotal.WithLabelValues(mode).Inc()
}

// RecordTrustedPeerRejection increments untrusted gateway header rejections.
func (r *Registry) RecordTrustedPeerRejection() {
	if r == nil {
		return
	}
	r.trustedPeerRejections.Inc()
}

// StatusClass maps HTTP status codes to low-cardinality buckets.
func StatusClass(code int) string {
	switch {
	case code >= 200 && code < 300:
		return "2xx"
	case code >= 400 && code < 500:
		return "4xx"
	case code >= 500 && code < 600:
		return "5xx"
	default:
		return "error"
	}
}

// StreamLabel returns a low-cardinality stream flag.
func StreamLabel(stream bool) string {
	if stream {
		return "true"
	}
	return "false"
}

// ProviderLabel normalizes provider names for metrics labels.
func ProviderLabel(format string) string {
	switch format {
	case "openai", "anthropic", "passthrough":
		return format
	default:
		return "passthrough"
	}
}

// Unregister stops the runtime collector (tests only).
func (r *Registry) Unregister() {
	if r == nil {
		return
	}
	r.Stop()
}

// ParseMetricValue extracts a counter value from an expfmt line (tests).
func ParseMetricValue(body, name, label string) (float64, bool) {
	prefix := name
	if label != "" {
		prefix = name + "{" + label + "}"
	}
	for _, line := range splitLines(body) {
		if len(line) == 0 || line[0] == '#' {
			continue
		}
		if len(line) >= len(prefix) && line[:len(prefix)] == prefix {
			parts := splitFields(line)
			if len(parts) < 2 {
				continue
			}
			v, err := strconv.ParseFloat(parts[len(parts)-1], 64)
			if err != nil {
				continue
			}
			return v, true
		}
	}
	return 0, false
}

func splitLines(s string) []string {
	var out []string
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == '\n' {
			out = append(out, s[start:i])
			start = i + 1
		}
	}
	if start < len(s) {
		out = append(out, s[start:])
	}
	return out
}

func splitFields(s string) []string {
	return stringsFields(s)
}

func stringsFields(s string) []string {
	var fields []string
	cur := ""
	for i := 0; i < len(s); i++ {
		if s[i] == ' ' || s[i] == '\t' {
			if cur != "" {
				fields = append(fields, cur)
				cur = ""
			}
			continue
		}
		cur += string(s[i])
	}
	if cur != "" {
		fields = append(fields, cur)
	}
	return fields
}
