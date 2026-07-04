package proxy

import (
	"bytes"
	"io"
	"log/slog"
	"net/http"
	"net/url"
)

type failoverTransport struct {
	next        http.RoundTripper
	fallbackURL *url.URL
	logger      *slog.Logger
	metrics     failoverMetrics
}

type failoverMetrics interface {
	RecordFailoverAttempt(success bool)
}

func (t *failoverTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	if t.fallbackURL == nil || t.fallbackURL.String() == "" {
		return t.next.RoundTrip(req)
	}

	var bodyBytes []byte
	if req.Body != nil {
		b, err := io.ReadAll(req.Body)
		if err != nil {
			return nil, err
		}
		bodyBytes = b
		req.Body = io.NopCloser(bytes.NewReader(bodyBytes))
	}

	res, err := t.next.RoundTrip(req)
	if !shouldFailover(err, res) {
		return res, err
	}

	t.logger.Warn("upstream failed, attempting transparent failover",
		"fallback", t.fallbackURL.String(),
		"original_err", err,
		"original_status", getStatusCode(res))

	if res != nil && res.Body != nil {
		res.Body.Close()
	}

	retryReq := req.Clone(req.Context())
	retryReq.URL.Scheme = t.fallbackURL.Scheme
	retryReq.URL.Host = t.fallbackURL.Host
	if t.fallbackURL.Path == "" || t.fallbackURL.Path == "/" {
		retryReq.URL.Path = req.URL.Path
	} else {
		retryReq.URL.Path = t.fallbackURL.Path
	}
	retryReq.Host = t.fallbackURL.Host

	if bodyBytes != nil {
		retryReq.Body = io.NopCloser(bytes.NewReader(bodyBytes))
	}

	retryRes, retryErr := t.next.RoundTrip(retryReq)
	if t.metrics != nil {
		t.metrics.RecordFailoverAttempt(retryErr == nil && retryRes != nil && !shouldFailover(nil, retryRes))
	}
	return retryRes, retryErr
}

func shouldFailover(err error, res *http.Response) bool {
	if err != nil {
		return true
	}
	if res == nil {
		return false
	}
	switch res.StatusCode {
	case http.StatusTooManyRequests, http.StatusBadGateway, http.StatusServiceUnavailable, http.StatusGatewayTimeout:
		return true
	default:
		return false
	}
}

func getStatusCode(res *http.Response) int {
	if res == nil {
		return 0
	}
	return res.StatusCode
}
