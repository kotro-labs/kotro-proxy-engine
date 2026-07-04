package proxy

import (
	"bytes"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/metrics"
)

func TestFailoverTransport(t *testing.T) {
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

	primaryServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusTooManyRequests)
		w.Write([]byte("rate limited"))
	}))
	defer primaryServer.Close()

	primaryURL, _ := url.Parse(primaryServer.URL)

	ft := &failoverTransport{
		next:        http.DefaultTransport,
		fallbackURL: fallbackURL,
		logger:      slog.Default(),
		metrics:     metrics.NewRegistry(),
	}

	req, _ := http.NewRequest(http.MethodPost, primaryURL.String(), bytes.NewBuffer([]byte("hello")))

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

func TestFailoverTransport_badGatewayUsesFallback(t *testing.T) {
	fallbackServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("fallback ok"))
	}))
	defer fallbackServer.Close()

	primaryServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
	}))
	defer primaryServer.Close()

	ft := &failoverTransport{
		next:        http.DefaultTransport,
		fallbackURL: mustParseURL(t, fallbackServer.URL),
		logger:      slog.Default(),
		metrics:     metrics.NewRegistry(),
	}

	req, _ := http.NewRequest(http.MethodPost, primaryServer.URL, bytes.NewBuffer([]byte("payload")))
	resp, err := ft.RoundTrip(req)
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected fallback 200, got %d", resp.StatusCode)
	}
}

func TestFailoverTransport_networkErrorUsesFallback(t *testing.T) {
	fallbackServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("recovered"))
	}))
	defer fallbackServer.Close()

	ft := &failoverTransport{
		next:        http.DefaultTransport,
		fallbackURL: mustParseURL(t, fallbackServer.URL),
		logger:      slog.Default(),
		metrics:     metrics.NewRegistry(),
	}

	req, _ := http.NewRequest(http.MethodPost, "http://127.0.0.1:1/unreachable", bytes.NewBuffer([]byte("payload")))
	resp, err := ft.RoundTrip(req)
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	if string(body) != "recovered" {
		t.Fatalf("expected fallback body, got %q", string(body))
	}
}

func mustParseURL(t *testing.T, raw string) *url.URL {
	t.Helper()
	u, err := url.Parse(raw)
	if err != nil {
		t.Fatal(err)
	}
	return u
}
