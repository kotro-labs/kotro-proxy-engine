use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::anthropic::{AnthropicTurn, MessagesRequest};
use crate::models::openai::{ChatCompletionRequest, ChatMessage};
use crate::cache::{
    normalize_text, normalize_tool_calls, normalize_value, CacheKeyStrategy,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnifiedRequest {
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<UnifiedMessage>,
    pub stream: bool,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnifiedMessage {
    pub role: String,
    pub content: Value,
    pub name: Option<String>,
    pub tool_calls: Option<Value>,
    pub tool_call_id: Option<String>,
}

impl UnifiedMessage {
    /// Clone with content / tool_calls normalized for hashing only.
    fn for_cache_key(&self) -> Self {
        Self {
            role: self.role.clone(),
            content: normalize_value(&self.content),
            name: self.name.clone(),
            tool_calls: self.tool_calls.as_ref().map(normalize_tool_calls),
            tool_call_id: self.tool_call_id.clone(),
        }
    }
}

impl UnifiedRequest {
    pub fn extract_prompt_state(&self) -> (String, String) {
        let system_prompt = self.system_prompt.clone();
        let mut latest_user = String::new();
        for msg in self.messages.iter().rev() {
            if msg.role == "user" {
                latest_user = content_text(&msg.content);
                break;
            }
        }
        (system_prompt, latest_user)
    }

    pub fn extract_cache_key_material(&self, strategy: CacheKeyStrategy, window_n: usize) -> Vec<u8> {
        let system_str = normalize_text(&self.system_prompt);
        match strategy {
            CacheKeyStrategy::FullDigest => {
                let messages: Vec<UnifiedMessage> =
                    self.messages.iter().map(UnifiedMessage::for_cache_key).collect();
                #[derive(Serialize)]
                struct FullPayload<'a> {
                    system: &'a str,
                    messages: &'a [UnifiedMessage],
                }
                serde_json::to_vec(&FullPayload {
                    system: &system_str,
                    messages: &messages,
                })
                .unwrap_or_default()
            }
            CacheKeyStrategy::LatestOnly => {
                let mut latest_user = String::new();
                for msg in self.messages.iter().rev() {
                    if msg.role == "user" {
                        latest_user = normalize_text(&content_text(&msg.content));
                        break;
                    }
                }
                format!("{system_str}||{latest_user}").into_bytes()
            }
            CacheKeyStrategy::WindowN => {
                let msg_len = self.messages.len();
                let start_idx = msg_len.saturating_sub(window_n);
                let window_messages: Vec<UnifiedMessage> = self.messages[start_idx..msg_len]
                    .iter()
                    .map(UnifiedMessage::for_cache_key)
                    .collect();

                #[derive(Serialize)]
                struct WindowPayload<'a> {
                    system: &'a str,
                    window: &'a [UnifiedMessage],
                }

                serde_json::to_vec(&WindowPayload {
                    system: &system_str,
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

// Implement translators

impl TryFrom<ChatCompletionRequest> for UnifiedRequest {
    type Error = &'static str;

    fn try_from(req: ChatCompletionRequest) -> Result<Self, Self::Error> {
        let mut system_prompt = String::new();
        let mut unified_messages = Vec::new();

        for msg in req.messages {
            if msg.role == "system" {
                if system_prompt.is_empty() {
                    system_prompt = content_text(&msg.content);
                } else {
                    system_prompt = format!("{}\n\n{}", system_prompt, content_text(&msg.content));
                }
            } else {
                unified_messages.push(UnifiedMessage {
                    role: msg.role,
                    content: msg.content,
                    name: msg.name,
                    tool_calls: msg.tool_calls,
                    tool_call_id: msg.tool_call_id,
                });
            }
        }

        Ok(UnifiedRequest {
            model: req.model,
            system_prompt,
            messages: unified_messages,
            stream: req.stream,
            max_tokens: None,
        })
    }
}

impl TryFrom<MessagesRequest> for UnifiedRequest {
    type Error = &'static str;

    fn try_from(req: MessagesRequest) -> Result<Self, Self::Error> {
        let system_prompt = content_text(&req.system);
        let mut unified_messages = Vec::new();

        for msg in req.messages {
            unified_messages.push(UnifiedMessage {
                role: msg.role,
                content: msg.content,
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        Ok(UnifiedRequest {
            model: req.model,
            system_prompt,
            messages: unified_messages,
            stream: req.stream,
            max_tokens: Some(req.max_tokens),
        })
    }
}

impl Into<ChatCompletionRequest> for UnifiedRequest {
    fn into(self) -> ChatCompletionRequest {
        let mut openai_messages = Vec::new();

        if !self.system_prompt.is_empty() {
            openai_messages.push(ChatMessage {
                role: "system".to_string(),
                content: Value::String(self.system_prompt),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        for msg in self.messages {
            openai_messages.push(ChatMessage {
                role: msg.role,
                content: msg.content,
                name: msg.name,
                tool_calls: msg.tool_calls,
                tool_call_id: msg.tool_call_id,
            });
        }

        ChatCompletionRequest {
            model: self.model,
            messages: openai_messages,
            stream: self.stream,
            max_completion_tokens: None,
        }
    }
}

impl Into<MessagesRequest> for UnifiedRequest {
    fn into(self) -> MessagesRequest {
        let mut anthropic_messages = Vec::new();

        for msg in self.messages {
            anthropic_messages.push(AnthropicTurn {
                role: msg.role,
                content: msg.content,
            });
        }

        MessagesRequest {
            model: self.model,
            system: if self.system_prompt.is_empty() {
                Value::Null
            } else {
                Value::String(self.system_prompt)
            },
            messages: anthropic_messages,
            stream: self.stream,
            max_tokens: self.max_tokens.unwrap_or(4096),
            thinking: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheKeyStrategy;
    use serde_json::json;

    fn msg(role: &str, content: &str) -> UnifiedMessage {
        UnifiedMessage {
            role: role.into(),
            content: json!(content),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn trailing_whitespace_does_not_change_window_key() {
        let a = UnifiedRequest {
            model: "gpt-4o".into(),
            system_prompt: "sys".into(),
            messages: vec![msg("user", "hello")],
            stream: true,
            max_tokens: None,
        };
        let b = UnifiedRequest {
            model: "gpt-4o".into(),
            system_prompt: "sys\n\n".into(),
            messages: vec![msg("user", "hello  \n")],
            stream: true,
            max_tokens: None,
        };
        assert_eq!(
            a.extract_cache_key_material(CacheKeyStrategy::WindowN, 4),
            b.extract_cache_key_material(CacheKeyStrategy::WindowN, 4)
        );
    }

    #[test]
    fn shuffled_tool_calls_same_window_key() {
        let tool_a = json!([
            {"id": "2", "type": "function", "function": {"name": "write", "arguments": "{}"}},
            {"id": "1", "type": "function", "function": {"name": "read", "arguments": "{}"}}
        ]);
        let tool_b = json!([
            {"id": "1", "type": "function", "function": {"name": "read", "arguments": "{}"}},
            {"id": "2", "type": "function", "function": {"name": "write", "arguments": "{}"}}
        ]);
        let a = UnifiedRequest {
            model: "gpt-4o".into(),
            system_prompt: "sys".into(),
            messages: vec![UnifiedMessage {
                role: "assistant".into(),
                content: json!(""),
                name: None,
                tool_calls: Some(tool_a),
                tool_call_id: None,
            }],
            stream: true,
            max_tokens: None,
        };
        let b = UnifiedRequest {
            model: "gpt-4o".into(),
            system_prompt: "sys".into(),
            messages: vec![UnifiedMessage {
                role: "assistant".into(),
                content: json!(""),
                name: None,
                tool_calls: Some(tool_b),
                tool_call_id: None,
            }],
            stream: true,
            max_tokens: None,
        };
        assert_eq!(
            a.extract_cache_key_material(CacheKeyStrategy::WindowN, 4),
            b.extract_cache_key_material(CacheKeyStrategy::WindowN, 4)
        );
    }
}
