//! Anthropic request models — mirrors `internal/models/anthropic.go`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::openai::content_text;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    #[serde(default)]
    pub system: Value,
    pub messages: Vec<AnthropicTurn>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicTurn {
    pub role: String,
    pub content: Value,
}

impl MessagesRequest {
    pub fn extract_prompt_state(&self) -> (String, String) {
        let system_prompt = content_text(&self.system);
        let mut latest_user = String::new();
        for msg in &self.messages {
            if msg.role == "user" {
                latest_user = content_text(&msg.content);
            }
        }
        (system_prompt, latest_user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_prompt_state() {
        let req: MessagesRequest = serde_json::from_value(json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 64,
            "stream": true,
            "system": "sys",
            "messages": [{"role": "user", "content": "ping"}]
        }))
        .unwrap();
        assert_eq!(req.extract_prompt_state(), ("sys".into(), "ping".into()));
    }
}
