# Runtime image packaged from the published Linux release binary
# (x86_64-unknown-linux-gnu). Used by .github/workflows/docker-publish.yml.
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY kotro-proxy /usr/local/bin/kotro-proxy
RUN chmod +x /usr/local/bin/kotro-proxy \
    && mkdir -p /data

# Bind all interfaces so published ports work (host defaults are loopback-only).
ENV KOTRO_LISTEN_ADDR=:8080 \
    KOTRO_METRICS_ADDR=0.0.0.0:9090 \
    KOTRO_CACHE_DB=/data/kotro-cache.db \
    KOTRO_UPSTREAM_URL=http://127.0.0.1:9000

WORKDIR /data
EXPOSE 8080 9090

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -sf http://127.0.0.1:8080/healthz || exit 1

CMD ["kotro-proxy"]
