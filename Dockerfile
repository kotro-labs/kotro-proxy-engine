# Multi-stage build: Go mock upstream + Rust proxy, minimal Alpine runtime.
# Both runtimes pre-fetch dependencies in isolated layers for cache efficiency.

# ==========================================
# STAGE 1: Go dependency fetch & mock build
# ==========================================
FROM golang:1.26-alpine AS go-builder

RUN apk add --no-cache ca-certificates git

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY cmd/mockupstream ./cmd/mockupstream

RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o /bin/mock-upstream ./cmd/mockupstream

# ==========================================
# STAGE 2: Rust dependency fetch & build
# ==========================================
FROM rust:1-alpine AS rust-builder

RUN apk add --no-cache musl-dev gcc pkgconfig perl make

WORKDIR /app/rust

# Workspace manifest + lockfile for dependency layer caching.
COPY rust/Cargo.toml rust/Cargo.lock ./
COPY rust/kotro-proxy/Cargo.toml kotro-proxy/Cargo.toml

# Dummy crate forces registry fetch without copying real sources.
RUN mkdir -p kotro-proxy/src \
    && printf 'pub fn dummy() {}\n' > kotro-proxy/src/lib.rs \
    && printf 'fn main() {}\n' > kotro-proxy/src/main.rs

ENV CARGO_TARGET_DIR=/app/rust/target
RUN cargo build --release -p kotro-proxy

# Replace stub with real sources and rebuild application code only.
RUN rm -rf kotro-proxy/src
COPY rust/kotro-proxy/src kotro-proxy/src
RUN touch kotro-proxy/src/main.rs kotro-proxy/src/lib.rs \
    && cargo build --release -p kotro-proxy

# ==========================================
# STAGE 3: Minimal production runtime
# ==========================================
FROM alpine:3.21 AS production

RUN apk add --no-cache ca-certificates curl

WORKDIR /root

COPY --from=go-builder /bin/mock-upstream .
COPY --from=rust-builder /app/rust/target/release/kotro-proxy .

RUN mkdir -p /root/data

# Bind all interfaces so `docker run -p …` and Compose port maps work.
# Host/native defaults remain loopback-only via Config::default().
ENV KOTRO_LISTEN_ADDR=:8080 \
    KOTRO_METRICS_ADDR=0.0.0.0:9090 \
    KOTRO_CACHE_DB=/root/data/kotro-cache.db

EXPOSE 8080 9000 9090

# Default: Rust high-performance proxy (override in Compose for mock-upstream).
CMD ["./kotro-proxy"]
