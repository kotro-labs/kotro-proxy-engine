//! OpenAI-compatible request models — mirrors `internal/models/openai.go`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::CacheKeyStrategy;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
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

    pub fn extract_cache_key_material(&self, strategy: CacheKeyStrategy, window_n: usize) -> Vec<u8> {
        match strategy {
            CacheKeyStrategy::FullDigest => serde_json::to_vec(&self.messages).unwrap_or_default(),
            CacheKeyStrategy::LatestOnly => {
                let mut system_prompt = String::new();
                for msg in &self.messages {
                    if msg.role == "system" {
                        system_prompt = content_text(&msg.content);
                        break;
                    }
                }
                let mut latest_user = String::new();
                for msg in self.messages.iter().rev() {
                    if msg.role == "user" {
                        latest_user = content_text(&msg.content);
                        break;
                    }
                }
                format!("{system_prompt}||{latest_user}").into_bytes()
            }
            CacheKeyStrategy::WindowN => {
                let mut system_prompt = String::new();
                for msg in &self.messages {
                    if msg.role == "system" {
                        system_prompt = content_text(&msg.content);
                        break;
                    }
                }

                let msg_len = self.messages.len();
                let start_idx = msg_len.saturating_sub(window_n);
                let window_messages: Vec<ChatMessage> = self.messages[start_idx..msg_len]
                    .iter()
                    .filter(|m| m.role != "system")
                    .cloned()
                    .collect();

                #[derive(Serialize)]
                struct WindowPayload<'a> {
                    system: &'a str,
                    window: &'a [ChatMessage],
                }

                serde_json::to_vec(&WindowPayload {
                    system: &system_prompt,
                    window: &window_messages,
                })
                .unwrap_or_default()
            }
        }
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

    #[test]
    fn window_n_splits_divergent_tool_histories() {
        let req_db = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: json!("System setup"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".into(),
                    content: json!("Run tests"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "tool".into(),
                    content: json!("Database output modification data"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("1".into()),
                },
                ChatMessage {
                    role: "user".into(),
                    content: json!("Run tests again"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: true,
        };

        let req_css = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: json!("System setup"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".into(),
                    content: json!("Run tests"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "tool".into(),
                    content: json!("CSS layout spacing updates"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("2".into()),
                },
                ChatMessage {
                    role: "user".into(),
                    content: json!("Run tests again"),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: true,
        };

        let mat_w1 = req_db.extract_cache_key_material(CacheKeyStrategy::WindowN, 4);
        let mat_w2 = req_css.extract_cache_key_material(CacheKeyStrategy::WindowN, 4);
        let key_w1 = crate::cache::generate_cache_key("scope-y", "gpt-4o", "openai", &mat_w1);
        let key_w2 = crate::cache::generate_cache_key("scope-y", "gpt-4o", "openai", &mat_w2);
        assert_ne!(
            key_w1, key_w2,
            "window_n must split keys over divergent tool histories"
        );
    }
}
