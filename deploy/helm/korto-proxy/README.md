# Korto Proxy Helm Chart (preview)

This chart is a **deployment scaffold**. A container image is not published yet —
build your own image from the Go proxy (`cmd/proxy`) or wire an internal registry
before installing in production.

```bash
helm install korto ./deploy/helm/korto-proxy \
  --set proxy.upstreamUrl=https://api.openai.com \
  --set proxy.fallbackUrl=https://backup-provider.example.com
```

Telemetry binds to `0.0.0.0:9090` inside the pod; restrict with NetworkPolicy in multi-tenant clusters.
