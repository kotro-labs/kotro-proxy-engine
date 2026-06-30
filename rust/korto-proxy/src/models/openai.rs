//! OpenAI-compatible request models — mirrors `internal/models/openai.go`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

impl ChatCompletionRequest {
    pub fn extract_prompt_state(&self) -> (String, String) {
        let mut system_prompt = String::new();
        let mut latest_user = String::new();
        for msg in &self.messages {
            match msg.role.as_str() {
                "system" => system_prompt = content_text(&msg.content),
                "user" => latest_user = content_text(&msg.content),
                _ => {}
            }
        }
        (system_prompt, latest_user)
    }
}

pub fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    part.get("text").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            })
            .collect(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_prompt_state() {
        let req: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "gpt-4",
            "stream": true,
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "hello"}
            ]
        }))
        .unwrap();
        assert_eq!(req.extract_prompt_state(), ("sys".into(), "hello".into()));
    }
}
