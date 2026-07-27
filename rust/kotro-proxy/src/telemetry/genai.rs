//! OpenTelemetry GenAI / MCP semantic-convention helpers.
//!
//! Content capture is **off by default**. These helpers only emit stable
//! identifiers (operation name, conversation id, tool name, token counts,
//! decision). Prompt/tool bodies are never attached to spans.
//!
//! Spec references (evolving):
//! - `gen_ai.operation.name`, `gen_ai.conversation.id`, `gen_ai.request.model`,
//!   `gen_ai.usage.input_tokens` / `output_tokens`
//! - MCP tool attributes: `gen_ai.tool.name`, `mcp.server.name`,
//!   `mcp.tool.decision` (Kotro extension)

use opentelemetry::{
    global,
    trace::{Span, Tracer},
    KeyValue,
};

/// Attribute keys used by Kotro GenAI/MCP spans.
pub mod attrs {
    pub const OPERATION: &str = "gen_ai.operation.name";
    pub const CONVERSATION: &str = "gen_ai.conversation.id";
    pub const REQUEST_MODEL: &str = "gen_ai.request.model";
    pub const SYSTEM: &str = "gen_ai.system";
    pub const INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
    pub const OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
    pub const TOOL_NAME: &str = "gen_ai.tool.name";
    pub const MCP_SERVER: &str = "mcp.server.name";
    pub const MCP_DECISION: &str = "mcp.tool.decision";
    pub const MCP_RULE: &str = "mcp.tool.rule_id";
}

/// Emit a GenAI chat/completion span. No-op when OTel is not configured
/// (the global noop tracer just drops the span).
pub fn record_llm_span(
    conversation_id: &str,
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    latency_ms: u64,
) {
    let tracer = global::tracer("kotro-proxy");
    let mut span = tracer.start("gen_ai.chat");
    span.set_attribute(KeyValue::new(attrs::OPERATION, "chat"));
    span.set_attribute(KeyValue::new(attrs::CONVERSATION, conversation_id.to_string()));
    span.set_attribute(KeyValue::new(attrs::SYSTEM, provider.to_string()));
    span.set_attribute(KeyValue::new(attrs::REQUEST_MODEL, model.to_string()));
    if input_tokens > 0 {
        span.set_attribute(KeyValue::new(attrs::INPUT_TOKENS, input_tokens as i64));
    }
    if output_tokens > 0 {
        span.set_attribute(KeyValue::new(attrs::OUTPUT_TOKENS, output_tokens as i64));
    }
    span.set_attribute(KeyValue::new("kotro.latency_ms", latency_ms as i64));
    span.end();
}

/// Emit an MCP / hook tool decision span.
pub fn record_tool_span(
    conversation_id: &str,
    server: &str,
    tool: &str,
    decision: &str,
    rule_id: &str,
) {
    let tracer = global::tracer("kotro-proxy");
    let mut span = tracer.start("mcp.tools.call");
    span.set_attribute(KeyValue::new(attrs::OPERATION, "execute_tool"));
    span.set_attribute(KeyValue::new(attrs::CONVERSATION, conversation_id.to_string()));
    span.set_attribute(KeyValue::new(attrs::TOOL_NAME, tool.to_string()));
    span.set_attribute(KeyValue::new(attrs::MCP_SERVER, server.to_string()));
    span.set_attribute(KeyValue::new(attrs::MCP_DECISION, decision.to_string()));
    if !rule_id.is_empty() {
        span.set_attribute(KeyValue::new(attrs::MCP_RULE, rule_id.to_string()));
    }
    span.end();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_do_not_panic_without_otel() {
        // No provider registered → noop tracer. Must not panic.
        record_llm_span("s1", "openai", "gpt-4o", 10, 20, 5);
        record_tool_span("s1", "files", "read_file", "deny", "deny-ssh-keys");
    }
}
