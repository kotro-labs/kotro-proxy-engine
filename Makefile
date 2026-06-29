.PHONY: build test bench mock proxy run dev load-test clean

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

clean:
	rm -rf bin/ kortolabs-cache.db
