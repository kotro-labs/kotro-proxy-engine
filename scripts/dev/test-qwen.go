//go:build ignore

package main

import (
	"bytes"
	"fmt"
	"io"
	"net/http"
	"os"
)

func main() {
	key := os.Getenv("DASHSCOPE_API_KEY")
	reqBody := `{"model":"qwen-plus","stream":true,"stream_options":{"include_usage":true},"messages":[{"role":"user","content":"hello"}]}`
	req, _ := http.NewRequest("POST", "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions", bytes.NewBuffer([]byte(reqBody)))
	req.Header.Set("Authorization", "Bearer "+key)
	req.Header.Set("Content-Type", "application/json")
	resp, _ := http.DefaultClient.Do(req)
	body, _ := io.ReadAll(resp.Body)
	fmt.Println(string(body))
}
