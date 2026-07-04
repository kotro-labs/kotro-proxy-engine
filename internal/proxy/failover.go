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
}

func (t *failoverTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	// If no fallback URL is configured, just proceed normally.
	if t.fallbackURL == nil || t.fallbackURL.String() == "" {
		return t.next.RoundTrip(req)
	}

	// We can only transparently retry if we have the full request body buffered in memory.
	// In our proxy architecture, the body is already read into memory and replaced with a bytes.Reader 
	// in ServeHTTP before hitting the ReverseProxy, so we can safely read and re-read it.
	var bodyBytes []byte
	if req.Body != nil {
		b, err := io.ReadAll(req.Body)
		if err != nil {
			return nil, err
		}
		bodyBytes = b
		// Restore body for the first attempt
		req.Body = io.NopCloser(bytes.NewReader(bodyBytes))
	}

	res, err := t.next.RoundTrip(req)

	// Determine if we need to failover.
	// We failover on network errors (err != nil) or HTTP 429 / 503 statuses.
	shouldFailover := err != nil || res.StatusCode == http.StatusTooManyRequests || res.StatusCode == http.StatusServiceUnavailable

	if shouldFailover {
		t.logger.Warn("upstream failed, attempting transparent failover", 
			"fallback", t.fallbackURL.String(),
			"original_err", err,
			"original_status", getStatusCode(res))

		// Close the failed response body to prevent leaks
		if res != nil && res.Body != nil {
			res.Body.Close()
		}

		// Clone the request for the retry
		retryReq := req.Clone(req.Context())
		
		// Redirect to the fallback URL
		retryReq.URL.Scheme = t.fallbackURL.Scheme
		retryReq.URL.Host = t.fallbackURL.Host
		
		// If the fallback URL has no path, just use req.URL.Path
		if t.fallbackURL.Path == "" || t.fallbackURL.Path == "/" {
			retryReq.URL.Path = req.URL.Path
		}
		retryReq.Host = t.fallbackURL.Host

		// Restore the body for the retry
		if bodyBytes != nil {
			retryReq.Body = io.NopCloser(bytes.NewReader(bodyBytes))
		}

		return t.next.RoundTrip(retryReq)
	}

	return res, err
}

func getStatusCode(res *http.Response) int {
	if res == nil {
		return 0
	}
	return res.StatusCode
}
