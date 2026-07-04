//! Cache matrix normalizer — mirrors `internal/optimizer/normalizer.go`.

use crate::models::openai::{ChatCompletionRequest, ChatMessage};

/// Reorders messages so context file dumps sort deterministically before hashing.
pub fn enforce_cache_matrix(req: &mut ChatCompletionRequest) {
    if req.messages.len() <= 1 {
        return;
    }

    let mut system_messages = Vec::new();
    let mut context_messages = Vec::new();
    let mut history_messages = Vec::new();
    let mut latest_user: Option<ChatMessage> = None;

    for (i, msg) in req.messages.iter().cloned().enumerate() {
        if i == req.messages.len() - 1 && msg.role == "user" {
            latest_user = Some(msg);
            continue;
        }

        if msg.role == "system" {
            system_messages.push(msg);
        } else if is_context_dump(&content_text(&msg.content)) {
            context_messages.push(msg);
        } else {
            history_messages.push(msg);
        }
    }

    context_messages.sort_by(|a, b| content_text(&a.content).cmp(&content_text(&b.content)));

    let mut rebuilt = Vec::with_capacity(req.messages.len());
    rebuilt.extend(system_messages);
    rebuilt.extend(context_messages);
    rebuilt.extend(history_messages);
    if let Some(user) = latest_user {
        rebuilt.push(user);
    }

    req.messages = rebuilt;
}

fn is_context_dump(text: &str) -> bool {
    if text.contains("<file") && text.contains("</file>") {
        return true;
    }
    text.trim_start().starts_with("```") && text.len() > 500
}

fn content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        _ => content.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: json!(content),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn sorts_context_files() {
        let mut req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                msg("system", "sys"),
                msg("user", "<file name=\"z.go\">z</file>"),
                msg("user", "<file name=\"a.go\">a</file>"),
                msg("user", "latest"),
            ],
            stream: false,
        };

        enforce_cache_matrix(&mut req);
        assert_eq!(content_text(&req.messages[1].content), "<file name=\"a.go\">a</file>");
        assert_eq!(content_text(&req.messages[3].content), "latest");
    }
}
