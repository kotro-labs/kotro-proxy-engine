// Package server boots the KortoLabs local HTTP listener and routes.
package server

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/kortolabs/proxy-engine/internal/cache"
	"github.com/kortolabs/proxy-engine/internal/config"
	"github.com/kortolabs/proxy-engine/internal/dashboard"
	"github.com/kortolabs/proxy-engine/internal/metrics"
	"github.com/kortolabs/proxy-engine/internal/proxy"
)

// Server wraps the HTTP listeners and their dependencies.
type Server struct {
	cfg         config.Config
	proxyHTTP   *http.Server
	metricsHTTP *http.Server
	cache       *cache.Store
	logger      *slog.Logger
	evictCancel context.CancelFunc
	metrics     *metrics.Registry
}

// New constructs a Server without starting it.
func New(cfg config.Config, logger *slog.Logger) (*Server, error) {
	store, err := cache.OpenWithOptions(cfg.CacheDBPath, cache.StoreOptions{
		TTL:               cfg.CacheTTL,
		EvictionInterval:  cfg.CacheEvictionInterval,
		EnableCompression: cfg.EnableCompression,
	})
	if err != nil {
		return nil, fmt.Errorf("server: cache: %w", err)
	}

	evictCtx, evictCancel := context.WithCancel(context.Background())
	store.StartEvictionWorker(evictCtx, cfg.CacheEvictionInterval, logger)

	var prom *metrics.Registry
	if cfg.EnableMetrics {
		prom = metrics.NewRegistry()
		store.SetMetrics(prom)
		prom.StartRuntimeCollector(15 * time.Second)
	}

	proxyOpts := proxy.OptionsFromConfig(cfg, logger, prom)

	chatHandler, err := proxy.NewHandler(proxyOpts, store, logger)
	if err != nil {
		_ = store.Close()
		return nil, fmt.Errorf("server: chat handler: %w", err)
	}

	anthropicHandler, err := proxy.NewAnthropicHandler(proxyOpts, store, logger)
	if err != nil {
		_ = store.Close()
		return nil, fmt.Errorf("server: anthropic handler: %w", err)
	}

	passthrough, err := proxy.NewPassthrough(cfg.UpstreamURL, logger, prom)
	if err != nil {
		_ = store.Close()
		return nil, fmt.Errorf("server: passthrough: %w", err)
	}

	proxyMux := http.NewServeMux()
	proxyMux.Handle("/v1/chat/completions", chatHandler)
	proxyMux.Handle("/v1/messages", anthropicHandler)
	proxyMux.Handle("/v1/", passthrough)
	proxyMux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"ok","service":"kortolabs-proxy"}`))
	})
	if cfg.EnablePprof && !cfg.EnableMetrics {
		registerPprof(proxyMux)
	}

	var metricsHTTP *http.Server
	if cfg.EnableMetrics && prom != nil {
		metricsMux := http.NewServeMux()
		metricsMux.Handle("/metrics", prom.Handler())
		dashboard.New(prom).Register(metricsMux)
		if cfg.EnablePprof {
			registerPprof(metricsMux)
		}
		metricsHTTP = &http.Server{
			Addr:         cfg.MetricsAddr,
			Handler:      telemetryLoggingMiddleware(logger, metricsMux),
			ReadTimeout:  cfg.ReadTimeout,
			WriteTimeout: cfg.WriteTimeout,
			IdleTimeout:  cfg.IdleTimeout,
		}
	}

	proxyHTTP := &http.Server{
		Addr:         cfg.ListenAddr,
		Handler:      proxyLoggingMiddleware(logger, proxyMux),
		ReadTimeout:  cfg.ReadTimeout,
		WriteTimeout: cfg.WriteTimeout,
		IdleTimeout:  cfg.IdleTimeout,
	}

	return &Server{
		cfg:         cfg,
		proxyHTTP:   proxyHTTP,
		metricsHTTP: metricsHTTP,
		cache:       store,
		logger:      logger,
		evictCancel: evictCancel,
		metrics:     prom,
	}, nil
}

func (s *Server) logStartup() {
	s.logger.Info("core LLM gateway listening",
		"addr", s.cfg.ListenAddr,
		"upstream", s.cfg.UpstreamURL,
		"cache_db", s.cache.Path(),
		"cache", s.cfg.EnableCache,
		"redaction", s.cfg.EnableRedaction,
		"compression", s.cfg.EnableCompression,
		"pprof", s.cfg.EnablePprof,
		"metrics", s.cfg.EnableMetrics,
		"cache_ttl", s.cfg.CacheTTL.String(),
		"cache_eviction", s.cfg.CacheEvictionInterval.String(),
	)
	if s.metricsHTTP != nil {
		s.logger.Info("telemetry and dashboard listening",
			"addr", s.cfg.MetricsAddr,
			"dashboard", true,
		)
	}
}

func proxyLoggingMiddleware(logger *slog.Logger, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/healthz" || strings.HasPrefix(r.URL.Path, "/debug/pprof") {
			next.ServeHTTP(w, r)
			return
		}
		logger.Info("request",
			"method", r.Method,
			"path", r.URL.Path,
			"provider_auth", strings.HasPrefix(r.Header.Get("Authorization"), "Bearer "),
		)
		next.ServeHTTP(w, r)
	})
}

func telemetryLoggingMiddleware(logger *slog.Logger, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/metrics" || r.URL.Path == "/dashboard" || r.URL.Path == "/api/dashboard" || strings.HasPrefix(r.URL.Path, "/debug/pprof") {
			next.ServeHTTP(w, r)
			return
		}
		logger.Info("telemetry request", "method", r.Method, "path", r.URL.Path)
		next.ServeHTTP(w, r)
	})
}

// ListenAndServe blocks until all listeners exit (typically after Shutdown).
func (s *Server) ListenAndServe() error {
	s.logStartup()

	errCh := make(chan error, 2)
	listeners := 1

	go func() {
		err := s.proxyHTTP.ListenAndServe()
		if errors.Is(err, http.ErrServerClosed) {
			err = nil
		}
		errCh <- err
	}()

	if s.metricsHTTP != nil {
		listeners++
		go func() {
			err := s.metricsHTTP.ListenAndServe()
			if errors.Is(err, http.ErrServerClosed) {
				err = nil
			}
			errCh <- err
		}()
	}

	for i := 0; i < listeners; i++ {
		if err := <-errCh; err != nil {
			return err
		}
	}
	return nil
}

// Shutdown gracefully stops all listeners and closes the cache store.
func (s *Server) Shutdown(ctx context.Context) error {
	if s.evictCancel != nil {
		s.evictCancel()
	}
	if s.metrics != nil {
		s.metrics.Stop()
	}

	var wg sync.WaitGroup
	var shutdownErr error
	var mu sync.Mutex

	record := func(err error) {
		if err == nil {
			return
		}
		mu.Lock()
		if shutdownErr == nil {
			shutdownErr = err
		}
		mu.Unlock()
	}

	wg.Add(1)
	go func() {
		defer wg.Done()
		record(s.proxyHTTP.Shutdown(ctx))
	}()

	if s.metricsHTTP != nil {
		wg.Add(1)
		go func() {
			defer wg.Done()
			record(s.metricsHTTP.Shutdown(ctx))
		}()
	}

	wg.Wait()

	if closeErr := s.cache.Close(); closeErr != nil && shutdownErr == nil {
		shutdownErr = closeErr
	}
	return shutdownErr
}

// Addr returns the configured proxy listen address.
func (s *Server) Addr() string { return s.cfg.ListenAddr }

// MetricsAddr returns the configured telemetry listen address.
func (s *Server) MetricsAddr() string { return s.cfg.MetricsAddr }

// HTTPHandler exposes the proxy mux for integration tests.
func (s *Server) HTTPHandler() http.Handler { return s.proxyHTTP.Handler }

// MetricsHTTPHandler exposes the telemetry mux for integration tests.
func (s *Server) MetricsHTTPHandler() http.Handler {
	if s.metricsHTTP == nil {
		return http.NotFoundHandler()
	}
	return s.metricsHTTP.Handler
}
