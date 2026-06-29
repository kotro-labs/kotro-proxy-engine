package compressor_test

import (
	"strings"
	"testing"

	"github.com/kortolabs/proxy-engine/internal/compressor"
)

func TestCompressStripsUnchangedBlocks(t *testing.T) {
	st := compressor.NewStateTracker()
	blockA := "MCP schema v1\nline1\nline2"
	blockB := "Directory tree:\n/src\n  main.go"

	first := blockA + "\n\n" + blockB
	out1, changed1 := st.CompressMessage(first)
	if changed1 {
		t.Fatal("first turn should not strip blocks")
	}
	if out1 != first {
		t.Fatalf("first turn content changed: %q", out1)
	}

	out2, changed2 := st.CompressMessage(first)
	if !changed2 {
		t.Fatal("second identical turn should strip redundant blocks")
	}
	if out2 != "" {
		t.Fatalf("expected empty after full dedup, got %q", out2)
	}
}

func TestCompressKeepsChangedBlocks(t *testing.T) {
	st := compressor.NewStateTracker()
	_, _ = st.CompressMessage("block one")

	updated := "block one\n\nblock two NEW"
	out, changed := st.CompressMessage(updated)
	if !changed {
		t.Fatal("expected change when new block added")
	}
	if !strings.Contains(out, "block two NEW") {
		t.Fatalf("new block missing: %q", out)
	}
}

func TestCompressReintroducesAfterTurnWithoutBlocks(t *testing.T) {
	st := compressor.NewStateTracker()
	_, _ = st.CompressMessage("alpha\n\nbeta")
	_, _ = st.CompressMessage("gamma")

	out, changed := st.CompressMessage("alpha\n\nbeta")
	if changed {
		t.Fatal("blocks not present in prior turn should not be treated as stripped")
	}
	if out != "alpha\n\nbeta" {
		t.Fatalf("expected full content back, got %q", out)
	}
}
