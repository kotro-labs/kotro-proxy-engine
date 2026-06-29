package proxy

import "net/http"

func forwardAuthHeaders(req *http.Request) {
	// ReverseProxy preserves incoming headers; strip hop-by-hop fields only.
	req.Header.Del("Connection")
	req.Header.Del("Keep-Alive")
	req.Header.Del("Proxy-Connection")
	req.Header.Del("TE")
	req.Header.Del("Trailer")
	req.Header.Del("Transfer-Encoding")
	req.Header.Del("Upgrade")
}

func forwardAnthropicHeaders(req *http.Request) {
	if key := req.Header.Get("x-api-key"); key != "" {
		req.Header.Set("x-api-key", key)
	}
	if ver := req.Header.Get("anthropic-version"); ver != "" {
		req.Header.Set("anthropic-version", ver)
	}
}
