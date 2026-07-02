package proxy

import (
	"bytes"
	"log/slog"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/config"
)

func TestOptionsFromConfig_LogsInvalidTrustedProxyCIDRs(t *testing.T) {
	var buf bytes.Buffer
	logger := slog.New(slog.NewTextHandler(&buf, nil))

	opts := OptionsFromConfig(config.Config{
		TrustUpstreamGateway: true,
		TrustedProxyCIDRs:    "not-a-cidr",
	}, logger, nil)

	if len(opts.Scope.TrustedProxyCIDRs) != 0 {
		t.Fatal("invalid CIDR config must fail safe to empty whitelist")
	}
	if !bytes.Contains(buf.Bytes(), []byte("invalid KORTO_TRUSTED_PROXY_CIDRS")) {
		t.Fatalf("expected config error log, got: %s", buf.String())
	}
}
