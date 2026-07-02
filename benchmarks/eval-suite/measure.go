//go:build ignore

// measure.go reports compressor savings for IDE-style workloads (offline, no proxy).
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/kortolabs/proxy-engine/internal/compressor"
)

type turnStat struct {
	Turn              int    `json:"turn"`
	InputBytes        int    `json:"input_bytes"`
	UpstreamBytes     int    `json:"upstream_bytes"`
	BlocksStripped    bool   `json:"blocks_stripped"`
	Cache             string `json:"cache"`
}

type workloadResult struct {
	Name           string     `json:"name"`
	Turns          []turnStat `json:"turns"`
	SavingsPctLast float64    `json:"savings_pct_last_turn"`
}

type compressionReport struct {
	W1ContextReload workloadResult `json:"w1_context_reload"`
	W2ToolDumps     workloadResult `json:"w2_tool_dumps"`
}

func main() {
	report := compressionReport{
		W1ContextReload: measureContextReload(),
		W2ToolDumps:     measureToolDumps(),
	}
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	if err := enc.Encode(report); err != nil {
		fmt.Fprintf(os.Stderr, "encode: %v\n", err)
		os.Exit(1)
	}
}

func measureContextReload() workloadResult {
	st := compressor.NewStateTracker(1000, time.Hour, nil)
	scope := compressor.Scope{TenantID: "eval", SessionID: "context-reload"}

	blockA := strings.Repeat("MCP schema v1\nline\n", 40)
	blockB := strings.Repeat("Directory tree:\n/src/main.go\n/pkg/cache.go\n", 30)
	static := strings.TrimSpace(blockA + "\n\n" + blockB)
	deltas := []string{
		"turn-1 user ask",
		"turn-2 follow-up",
		"turn-3 refine",
		"turn-4 patch",
		"turn-5 test",
		"turn-6 lint",
		"turn-7 commit msg",
		"turn-8 review",
		"turn-9 deploy",
		"turn-10 ship",
	}

	var turns []turnStat
	for i, delta := range deltas {
		payload := static + "\n\n" + delta
		out, stripped := st.CompressMessage(scope, payload)
		turns = append(turns, turnStat{
			Turn:           i + 1,
			InputBytes:     len(payload),
			UpstreamBytes:  len(out),
			BlocksStripped: stripped,
			Cache:          cacheLabel(i),
		})
	}

	last := turns[len(turns)-1]
	savings := pctSaved(last.InputBytes, last.UpstreamBytes)

	return workloadResult{
		Name:           "context_reload_storm",
		Turns:          turns,
		SavingsPctLast: savings,
	}
}

func measureToolDumps() workloadResult {
	st := compressor.NewStateTracker(1000, time.Hour, nil)
	scope := compressor.Scope{TenantID: "eval", SessionID: "tool-dumps"}

	toolDump := strings.Repeat("grep -R \"func main\" ./\n./cmd/proxy/main.go:12\n./cmd/mockupstream/main.go:18\n", 80)
	dirListing := strings.Repeat("ls -la src/\n-rw-r--r-- main.go\n-rw-r--r-- cache.go\n", 50)
	static := strings.TrimSpace(toolDump + "\n\n" + dirListing)
	deltas := []string{
		"analyze stack trace",
		"summarize findings",
		"propose fix",
	}

	var turns []turnStat
	for i, delta := range deltas {
		payload := static + "\n\n" + delta
		out, stripped := st.CompressMessage(scope, payload)
		turns = append(turns, turnStat{
			Turn:           i + 1,
			InputBytes:     len(payload),
			UpstreamBytes:  len(out),
			BlocksStripped: stripped,
			Cache:          cacheLabel(i),
		})
	}

	last := turns[len(turns)-1]
	return workloadResult{
		Name:           "tool_output_dumps",
		Turns:          turns,
		SavingsPctLast: pctSaved(last.InputBytes, last.UpstreamBytes),
	}
}

func cacheLabel(turnIdx int) string {
	if turnIdx == 0 {
		return "MISS"
	}
	return "n/a"
}

func pctSaved(input, upstream int) float64 {
	if input == 0 {
		return 0
	}
	return float64(input-upstream) / float64(input) * 100
}
