.PHONY: build test bench mock proxy run dev load-test cancel-audit rust-cancel-audit audit eval-suite demo-savings demo-injection update-homebrew-shas post-release-homebrew go-live package-extension sync-brand-icon clean docker-build docker-up docker-down

build: proxy mock

proxy:
	cd rust && CARGO_TARGET_DIR=../bin/rust-target cargo build --release -p kotro-proxy
	cp bin/rust-target/release/kotro-proxy bin/kotro-proxy
	# Fresh `cp` of an adhoc-signed Mach-O can hang under dyld on macOS; re-sign.
	codesign -s - --force bin/kotro-proxy >/dev/null 2>&1 || true

mock:
	go build -o bin/mock-upstream ./cmd/mockupstream

test:
	go test ./...

bench:
	go test -bench=. -benchmem ./internal/proxy/...

run: build
	@echo "Start mock upstream: bin/mock-upstream"
	@echo "Start proxy: KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 bin/kotro-proxy"

dev: build
	bash scripts/dev-up.sh

load-test: build
	bash scripts/bench/run.sh $(SCENARIO)

cancel-audit: build
	bash benchmarks/run_audit.sh

rust-test:
	cd rust && cargo test

rust-build:
	cd rust && CARGO_TARGET_DIR=target cargo build --release -p kotro-proxy

rust-run:
	cd rust && cargo run -p kotro-proxy

rust-cancel-audit:
	bash benchmarks/run_rust_audit.sh

# Run both audits sequentially (never in parallel — they share :8080/:9000).
audit: cancel-audit rust-cancel-audit

eval-suite:
	go run benchmarks/eval-suite/main.go

# Offline savings demo — no API keys required; uses bundled mock upstream.
# Screenshot the terminal output for Show HN / README.
demo-savings:
	bash scripts/demo-savings.sh

# Honest injection demo: poisoned tool result → warn header → HTTP 400 block.
# Leave http://127.0.0.1:9090/dashboard open for the recording / screenshot.
demo-injection:
	bash scripts/demo-injection.sh

update-homebrew-shas:
	bash scripts/update-homebrew-shas.sh $(VERSION)

post-release-homebrew:
	bash scripts/post-release-homebrew.sh $(VERSION)

go-live:
	bash scripts/go-live.sh $(VERSION)

package-extension:
	bash scripts/package-extension-local.sh $(ARTIFACTS_DIR)

sync-brand-icon:
	bash scripts/sync-brand-icon.sh

clean:
	rm -rf bin/ kotro-cache.db benchmarks/.audit-logs

docker-build:
	docker-compose build

docker-up:
	docker-compose up

docker-down:
	docker-compose down
