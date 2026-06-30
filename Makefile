.PHONY: build test bench mock proxy run dev load-test cancel-audit clean

build: proxy mock

proxy:
	go build -o bin/kortolabs-proxy ./cmd/proxy

mock:
	go build -o bin/mock-upstream ./cmd/mockupstream

test:
	go test ./...

bench:
	go test -bench=. -benchmem ./internal/proxy/...

run: build
	@echo "Start mock upstream: bin/mock-upstream"
	@echo "Start proxy: KORTO_UPSTREAM_URL=http://127.0.0.1:9000 bin/kortolabs-proxy"

dev: build
	bash scripts/dev-up.sh

load-test: build
	bash scripts/bench/run.sh $(SCENARIO)

cancel-audit: build
	bash benchmarks/run_audit.sh

clean:
	rm -rf bin/ kortolabs-cache.db

rust-test:
	cd rust && cargo test

rust-build:
	cd rust && CARGO_TARGET_DIR=target cargo build --release -p korto-proxy

rust-run:
	cd rust && cargo run -p korto-proxy

rust-cancel-audit:
	bash benchmarks/run_rust_audit.sh
