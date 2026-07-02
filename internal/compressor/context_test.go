package compressor_test

import (
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/kortolabs/proxy-engine/internal/compressor"
)

func tracker() *compressor.StateTracker {
	return compressor.NewStateTracker(1000, time.Hour)
}

func scope(tenant, session string) compressor.Scope {
	return compressor.Scope{TenantID: tenant, SessionID: session}
}

func TestCompressStripsUnchangedBlocks(t *testing.T) {
	st := tracker()
	s := scope("tenant-a", "session-1")
	blockA := "MCP schema v1\nline1\nline2"
	blockB := "Directory tree:\n/src\n  main.go"

	first := blockA + "\n\n" + blockB
	out1, changed1 := st.CompressMessage(s, first)
	if changed1 {
		t.Fatal("first turn should not strip blocks")
	}
	if out1 != first {
		t.Fatalf("first turn content changed: %q", out1)
	}

	out2, changed2 := st.CompressMessage(s, first)
	if !changed2 {
		t.Fatal("second identical turn should strip redundant blocks")
	}
	if out2 != "" {
		t.Fatalf("expected empty after full dedup, got %q", out2)
	}
}

func TestCompressKeepsChangedBlocks(t *testing.T) {
	st := tracker()
	s := scope("tenant-a", "session-1")
	_, _ = st.CompressMessage(s, "block one")

	updated := "block one\n\nblock two NEW"
	out, changed := st.CompressMessage(s, updated)
	if !changed {
		t.Fatal("expected change when new block added")
	}
	if !strings.Contains(out, "block two NEW") {
		t.Fatalf("new block missing: %q", out)
	}
}

func TestCompressReintroducesAfterTurnWithoutBlocks(t *testing.T) {
	st := tracker()
	s := scope("tenant-a", "session-1")
	_, _ = st.CompressMessage(s, "alpha\n\nbeta")
	_, _ = st.CompressMessage(s, "gamma")

	out, changed := st.CompressMessage(s, "alpha\n\nbeta")
	if changed {
		t.Fatal("blocks not present in prior turn should not be treated as stripped")
	}
	if out != "alpha\n\nbeta" {
		t.Fatalf("expected full content back, got %q", out)
	}
}

func TestCompressIsolatesTenantSessions(t *testing.T) {
	st := tracker()
	payload := "shared block\n\ncontext"

	tenantA := scope("tenant-a", "session-1")
	tenantB := scope("tenant-b", "session-1")

	_, _ = st.CompressMessage(tenantA, payload)
	outB, changedB := st.CompressMessage(tenantB, payload)
	if changedB {
		t.Fatalf("tenant B first turn should not inherit tenant A state, got %q", outB)
	}

	outA, changedA := st.CompressMessage(tenantA, payload)
	if !changedA {
		t.Fatal("tenant A second turn should dedupe within its own scope")
	}
	if outA != "" {
		t.Fatalf("expected empty after tenant A dedup, got %q", outA)
	}
}

func TestCompressConcurrentScopes(t *testing.T) {
	st := tracker()
	payload := "alpha\n\nbeta"

	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func(id int) {
			defer wg.Done()
			s := scope("tenant", "session-"+strconv.Itoa(id))
			_, _ = st.CompressMessage(s, payload)
			out, changed := st.CompressMessage(s, payload)
			if !changed || out != "" {
				t.Errorf("scope %d: expected full dedup on second turn, changed=%v out=%q", id, changed, out)
			}
		}(i)
	}
	wg.Wait()
}
