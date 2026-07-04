package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/kortolabs/proxy-engine/internal/models"
	"github.com/kortolabs/proxy-engine/internal/optimizer"
)

func main() {
	apiKey := os.Getenv("DEEPSEEK_API_KEY")
	if apiKey == "" {
		fmt.Println("Error: DEEPSEEK_API_KEY is required in environment.")
		os.Exit(1)
	}

	fmt.Println("DeepSeek Cache Optimization Eval Suite")
	fmt.Println("======================================")

	out, err := os.OpenFile("benchmarks/eval-suite/RESULTS.md", os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0644)
	if err != nil {
		fmt.Printf("Failed to create RESULTS.md: %v\n", err)
		os.Exit(1)
	}
	defer out.Close()

	out.WriteString("# DeepSeek Server-Side Cache Benchmarks\n\n")

	system := models.ChatMessage{
		Role:    "system",
		Content: mustFlexText("You are an expert systems engineer. Adhere strictly to the requested architecture."),
	}

	// 5000 character mock codebase dump
	codeDump := fmt.Sprintf("<file name=\"core.go\">\npackage core\n\n%s\n</file>", strings.Repeat("// complex logic simulation loop\nfunc mock() {}\n", 200))

	var history []models.ChatMessage

	// Turn 1
	turn1Query := "What is the time complexity of the core loop?"
	runTurn(1, apiKey, out, system, history, codeDump, turn1Query)

	// Turn 2
	history = append(history, models.ChatMessage{Role: "user", Content: mustFlexText(turn1Query)})
	history = append(history, models.ChatMessage{Role: "assistant", Content: mustFlexText("The time complexity is O(N).")})
	turn2Query := "Can you optimize it to O(1)?"
	runTurn(2, apiKey, out, system, history, codeDump, turn2Query)

	// Turn 3
	history = append(history, models.ChatMessage{Role: "user", Content: mustFlexText(turn2Query)})
	history = append(history, models.ChatMessage{Role: "assistant", Content: mustFlexText("Yes, by using a hash map.")})
	turn3Query := "Write the hash map implementation."
	runTurn(3, apiKey, out, system, history, codeDump, turn3Query)
	
	fmt.Println("\nBenchmark complete. Results saved to benchmarks/eval-suite/RESULTS.md")
}

func mustFlexText(text string) models.FlexContent {
	b, _ := json.Marshal(text)
	var fc models.FlexContent
	fc.UnmarshalJSON(b)
	return fc
}

func runTurn(turn int, apiKey string, out io.Writer, system models.ChatMessage, history []models.ChatMessage, contextDump string, query string) {
	req := &models.ChatCompletionRequest{
		Model:  "deepseek-chat", // DeepSeek standard endpoint model (deepseek-v4-flash internally depending on routing)
		Stream: false,
	}

	// Simulate sloppy IDE formatting: user query, then context, then history, then system
	req.Messages = append(req.Messages, models.ChatMessage{Role: "user", Content: mustFlexText(query)})
	req.Messages = append(req.Messages, models.ChatMessage{Role: "user", Content: mustFlexText(contextDump)})
	req.Messages = append(req.Messages, history...)
	req.Messages = append(req.Messages, system)

	fmt.Printf("\n--- Turn %d ---\n", turn)
	fmt.Printf("Before Normalization: System block is at index %d\n", findSystem(req.Messages))

	// Enforce the strict cache matrix
	optimizer.EnforceCacheMatrix(req)

	fmt.Printf("After Normalization : System block is at index %d\n", findSystem(req.Messages))

	bodyBytes, _ := json.Marshal(req)

	httpReq, _ := http.NewRequest("POST", "https://api.deepseek.com/chat/completions", bytes.NewBuffer(bodyBytes))
	httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	httpReq.Header.Set("Content-Type", "application/json")

	client := &http.Client{Timeout: 60 * time.Second}
	resp, err := client.Do(httpReq)
	if err != nil {
		fmt.Printf("Network error: %v\n", err)
		return
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		fmt.Printf("API Error: %s\n", resp.Status)
		body, _ := io.ReadAll(resp.Body)
		fmt.Printf("Body: %s\n", string(body))
		return
	}

	var result struct {
		Usage struct {
			PromptTokens          int `json:"prompt_tokens"`
			PromptCacheHitTokens  int `json:"prompt_cache_hit_tokens"`
			PromptCacheMissTokens int `json:"prompt_cache_miss_tokens"`
			CompletionTokens      int `json:"completion_tokens"`
		} `json:"usage"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		fmt.Printf("JSON Decode error: %v\n", err)
		return
	}

	fmt.Printf("Prompt Tokens     : %d\n", result.Usage.PromptTokens)
	fmt.Printf("Cache Hits        : %d\n", result.Usage.PromptCacheHitTokens)
	fmt.Printf("Cache Misses      : %d\n", result.Usage.PromptCacheMissTokens)

	hitRatio := 0.0
	if result.Usage.PromptTokens > 0 {
		hitRatio = float64(result.Usage.PromptCacheHitTokens) / float64(result.Usage.PromptTokens) * 100
	}

	out.Write([]byte(fmt.Sprintf("### Turn %d\n", turn)))
	out.Write([]byte(fmt.Sprintf("- **Prompt Tokens**: %d\n", result.Usage.PromptTokens)))
	out.Write([]byte(fmt.Sprintf("- **Cache Hit Tokens**: %d\n", result.Usage.PromptCacheHitTokens)))
	out.Write([]byte(fmt.Sprintf("- **Cache Miss Tokens**: %d\n", result.Usage.PromptCacheMissTokens)))
	out.Write([]byte(fmt.Sprintf("- **Hit Ratio**: %.2f%%\n\n", hitRatio)))
}

func findSystem(msgs []models.ChatMessage) int {
	for i, m := range msgs {
		if m.Role == "system" {
			return i
		}
	}
	return -1
}
