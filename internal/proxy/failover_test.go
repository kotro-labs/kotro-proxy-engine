package proxy

import (
	"bytes"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
)

func TestFailoverTransport(t *testing.T) {
	// Setup the mock fallback server
	fallbackServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if string(body) != "hello" {
			t.Errorf("expected body 'hello', got %s", string(body))
		}
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("fallback success"))
	}))
	defer fallbackServer.Close()

	fallbackURL, _ := url.Parse(fallbackServer.URL)

	// Setup the mock primary server (which will fail with 429)
	primaryServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if string(body) != "hello" {
			t.Errorf("expected body 'hello', got %s", string(body))
		}
		w.WriteHeader(http.StatusTooManyRequests)
		w.Write([]byte("rate limited"))
	}))
	defer primaryServer.Close()

	primaryURL, _ := url.Parse(primaryServer.URL)

	ft := &failoverTransport{
		next:        http.DefaultTransport,
		fallbackURL: fallbackURL,
		logger:      slog.Default(),
	}

	req, _ := http.NewRequest("POST", primaryURL.String(), bytes.NewBuffer([]byte("hello")))
	
	resp, err := ft.RoundTrip(req)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 OK from fallback, got %d", resp.StatusCode)
	}

	respBody, _ := io.ReadAll(resp.Body)
	if string(respBody) != "fallback success" {
		t.Errorf("expected 'fallback success', got %s", string(respBody))
	}
}
