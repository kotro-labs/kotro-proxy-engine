package optimizer

import (
	"sort"
	"strings"

	"github.com/kortolabs/proxy-engine/internal/models"
)

// EnforceCacheMatrix intercepts the JSON message payloads and enforces structural determinism.
// Array Priority:
// 1. Index 0: System Instructions
// 2. Index 1: Tool definitions (implicitly handled by OpenAI structure outside messages, or system prompt)
// 3. Index 2: Context Files (Sorted alphanumerically)
// 4. Index 3: Historical Message Chain
// 5. Suffix: Latest User Query
func EnforceCacheMatrix(req *models.ChatCompletionRequest) {
	if len(req.Messages) <= 1 {
		return
	}

	var systemMessages []models.ChatMessage
	var contextMessages []models.ChatMessage
	var historyMessages []models.ChatMessage
	var latestUser *models.ChatMessage

	for i, msg := range req.Messages {
		if i == len(req.Messages)-1 && msg.Role == "user" {
			latestUser = &msg
			continue
		}

		if msg.Role == "system" {
			systemMessages = append(systemMessages, msg)
		} else if isContextDump(msg.Content.Text()) {
			contextMessages = append(contextMessages, msg)
		} else {
			historyMessages = append(historyMessages, msg)
		}
	}

	// Sort context files alphanumerically by filename/content to prevent IDE from shuffling order
	sort.SliceStable(contextMessages, func(i, j int) bool {
		return contextMessages[i].Content.Text() < contextMessages[j].Content.Text()
	})

	var rebuilt []models.ChatMessage
	rebuilt = append(rebuilt, systemMessages...)
	rebuilt = append(rebuilt, contextMessages...)
	rebuilt = append(rebuilt, historyMessages...)

	if latestUser != nil {
		rebuilt = append(rebuilt, *latestUser)
	}

	req.Messages = rebuilt
}

func isContextDump(text string) bool {
	if strings.Contains(text, "<file") && strings.Contains(text, "</file>") {
		return true
	}
	// Fallback for markdown codeblock dumps over 500 chars that aren't history
	if strings.HasPrefix(strings.TrimSpace(text), "```") && len(text) > 500 {
		return true
	}
	return false
}
