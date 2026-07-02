.PHONY: build test bench mock proxy run dev load-test cancel-audit rust-cancel-audit audit eval-suite update-homebrew-shas post-release-homebrew go-live package-extension clean docker-build docker-up docker-down

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

rust-test:
	cd rust && cargo test

rust-build:
	cd rust && CARGO_TARGET_DIR=target cargo build --release -p korto-proxy

rust-run:
	cd rust && cargo run -p korto-proxy

rust-cancel-audit:
	bash benchmarks/run_rust_audit.sh

# Run both audits sequentially (never in parallel — they share :8080/:9000).
audit: cancel-audit rust-cancel-audit

eval-suite: build
	bash benchmarks/eval-suite/run.sh

update-homebrew-shas:
	bash scripts/update-homebrew-shas.sh $(VERSION)

post-release-homebrew:
	bash scripts/post-release-homebrew.sh $(VERSION)

go-live:
	bash scripts/go-live.sh $(VERSION)

package-extension:
	bash scripts/package-extension-local.sh $(ARTIFACTS_DIR)

clean:
	rm -rf bin/ kortolabs-cache.db benchmarks/.audit-logs

docker-build:
	docker-compose build

docker-up:
	docker-compose up

docker-down:
	docker-compose down
