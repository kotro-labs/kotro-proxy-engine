package proxy

import (
	"time"

	"github.com/kortolabs/proxy-engine/internal/guardrail"
	"github.com/kortolabs/proxy-engine/internal/metrics"
)

type ctxKeyUpstreamStart struct{}

type requestObserver struct {
	metrics     *metrics.Registry
	provider    string
	route       string
	stream      string
	cacheStatus string
	started     time.Time
}

func newRequestObserver(m *metrics.Registry, provider, route string) *requestObserver {
	return &requestObserver{
		metrics:     m,
		provider:    provider,
		route:       route,
		cacheStatus: "bypass",
		stream:      "false",
		started:     time.Now(),
	}
}

func (o *requestObserver) setStream(stream bool) {
	if o == nil {
		return
	}
	o.stream = metrics.StreamLabel(stream)
}

func (o *requestObserver) setCacheStatus(status string) {
	if o == nil {
		return
	}
	o.cacheStatus = status
}

func (o *requestObserver) finish() {
	if o == nil || o.metrics == nil {
		return
	}
	o.metrics.RecordRequest(o.provider, o.route, o.stream, o.cacheStatus, time.Since(o.started))
}

func recordScopeMetrics(m *metrics.Registry, meta ScopeMeta) {
	if m == nil {
		return
	}
	m.RecordScopeMode(meta.Mode)
	if meta.TrustedPeerRejected {
		m.RecordTrustedPeerRejection()
	}
}

func recordRedactionMetrics(m *metrics.Registry, rm *guardrail.RedactionMap) {
	if m == nil || rm == nil {
		return
	}
	for pattern, count := range rm.PatternCounts() {
		for i := 0; i < count; i++ {
			m.RecordRedaction(pattern)
		}
	}
}

func recordUpstreamMetrics(m *metrics.Registry, provider string, statusCode int, started time.Time) {
	if m == nil || started.IsZero() {
		return
	}
	m.RecordUpstream(provider, statusCode, time.Since(started))
}
