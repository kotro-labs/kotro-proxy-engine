# Kotro Proxy Helm Chart (preview)

Published image: `ghcr.io/kotro-labs/kotro-proxy` (see GitHub Packages / release workflow
`Publish container image (GHCR)`). Override `image.tag` to pin a release.

```bash
helm install kotro ./deploy/helm/kotro-proxy \
  --set proxy.upstreamUrl=https://api.openai.com \
  --set proxy.fallbackUrl=https://backup-provider.example.com
```

Telemetry binds to `0.0.0.0:9090` inside the pod; restrict with NetworkPolicy in multi-tenant clusters.
