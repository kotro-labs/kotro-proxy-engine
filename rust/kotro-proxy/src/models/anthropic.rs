//! Anthropic request models — mirrors `internal/models/anthropic.go`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::CacheKeyStrategy;
use super::openai::content_text;

/// Anthropic extended-thinking configuration.
///
/// Injected or enforced by the reasoning budget controller when
/// `KOTRO_MAX_THINKING_TOKENS` is set and the model is a known reasoning model.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThinkingConfig {
    /// Always `"enabled"` when Kotro injects this field.
    #[serde(rename = "type")]
    pub thinking_type: String,
    /// Maximum thinking tokens Anthropic may use for this request.
    pub budget_tokens: u64,
}

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
    /// Extended-thinking budget. `None` = not set (Anthropic default: no thinking).
    /// The reasoning budget controller injects or caps this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
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

    pub fn extract_cache_key_material(&self, strategy: CacheKeyStrategy, window_n: usize) -> Vec<u8> {
        let system_str = content_text(&self.system);

        match strategy {
            CacheKeyStrategy::FullDigest => {
                #[derive(Serialize)]
                struct FullPayload<'a> {
                    system: &'a str,
                    messages: &'a [AnthropicTurn],
                }
                serde_json::to_vec(&FullPayload {
                    system: &system_str,
                    messages: &self.messages,
                })
                .unwrap_or_default()
            }
            CacheKeyStrategy::LatestOnly => {
                let mut latest_user = String::new();
                for msg in self.messages.iter().rev() {
                    if msg.role == "user" {
                        latest_user = content_text(&msg.content);
                        break;
                    }
                }
                format!("{system_str}||{latest_user}").into_bytes()
            }
            CacheKeyStrategy::WindowN => {
                let msg_len = self.messages.len();
                let start_idx = msg_len.saturating_sub(window_n);
                let window_messages = &self.messages[start_idx..msg_len];

                #[derive(Serialize)]
                struct WindowPayload<'a> {
                    system: &'a str,
                    window: &'a [AnthropicTurn],
                }

                serde_json::to_vec(&WindowPayload {
                    system: &system_str,
                    window: window_messages,
                })
                .unwrap_or_default()
            }
        }
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
