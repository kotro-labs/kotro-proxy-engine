//! Reasoning model budget controller.
//!
//! Intercepts requests to known reasoning models (Claude Opus 4.x, o3/o1 family)
//! and enforces a configurable thinking-token cap before the request leaves the
//! machine. This prevents a single complex session from silently burning $50+.
//!
//! ## Configuration
//!
//! | Env var | Default | Description |
//! |---------|---------|-------------|
//! | `KOTRO_MAX_THINKING_TOKENS` | `0` | Cap thinking/reasoning tokens (0 = no cap) |
//! | `KOTRO_REASONING_BLOCK` | `false` | Block reasoning model requests entirely (HTTP 403) |
//!
//! ## What it does
//!
//! **Anthropic (Opus 4.x):**
//! Injects `"thinking": {"type": "enabled", "budget_tokens": N}` into the request body.
//! If `thinking.budget_tokens` is already set and exceeds the cap, it is reduced.
//! If it is already at or below the cap, the field is left unchanged.
//!
//! **OpenAI (o1/o3 family):**
//! Sets or reduces `max_completion_tokens` to cap total token spend.
//!
//! ## Reasoning model detection
//!
//! Anthropic: any model whose name contains `claude-opus-4` or `opus-4`.
//! OpenAI: `o1`, `o1-mini`, `o1-preview`, `o3`, `o3-mini`, `o3-pro`, and
//! any future `o1-*` / `o3-*` prefixed model names.

use crate::models::anthropic::{MessagesRequest, ThinkingConfig};
use crate::models::openai::ChatCompletionRequest;

/// Returns `true` when the model name indicates an Anthropic extended-thinking model.
///
/// Matches `claude-opus-4`, `claude-opus-4-8`, and any future `*opus-4*` variant.
pub fn is_anthropic_reasoning_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("claude-opus-4") || m.contains("opus-4")
}

/// Returns `true` when the model name indicates an OpenAI reasoning model.
///
/// Matches `o1`, `o1-mini`, `o1-preview`, `o3`, `o3-mini`, `o3-pro`, and any
/// future `o1-*` or `o3-*` prefixed variants.
pub fn is_openai_reasoning_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m == "o1"
        || m == "o3"
        || m.starts_with("o1-")
        || m.starts_with("o3-")
}

/// The outcome of applying a reasoning budget to a request.
#[derive(Debug, PartialEq)]
pub enum ReasoningOutcome {
    /// Budget was injected or reduced to `cap` tokens.
    Capped { cap: u64 },
    /// The model already had a budget ≤ the cap; left unchanged at `existing` tokens.
    AlreadyWithinBudget { existing: u64 },
    /// Model is not a reasoning model; no change made.
    NotApplicable,
}

/// Apply the reasoning token cap to an Anthropic `MessagesRequest`.
///
/// - `thinking` absent → injects `{"type":"enabled","budget_tokens":cap}`.
/// - `thinking.budget_tokens` > `cap` → reduces to `cap`.
/// - `thinking.budget_tokens` ≤ `cap` → no-op (already conservative).
pub fn apply_anthropic_reasoning_budget(
    req: &mut MessagesRequest,
    cap: u64,
) -> ReasoningOutcome {
    if !is_anthropic_reasoning_model(&req.model) {
        return ReasoningOutcome::NotApplicable;
    }
    match &req.thinking {
        None => {
            req.thinking = Some(ThinkingConfig {
                thinking_type: "enabled".into(),
                budget_tokens: cap,
            });
            ReasoningOutcome::Capped { cap }
        }
        Some(t) if t.budget_tokens > cap => {
            req.thinking = Some(ThinkingConfig {
                thinking_type: "enabled".into(),
                budget_tokens: cap,
            });
            ReasoningOutcome::Capped { cap }
        }
        Some(t) => ReasoningOutcome::AlreadyWithinBudget {
            existing: t.budget_tokens,
        },
    }
}

/// Apply the reasoning token cap to an OpenAI `ChatCompletionRequest`.
///
/// - `max_completion_tokens` absent → sets it to `cap`.
/// - `max_completion_tokens` > `cap` → reduces to `cap`.
/// - `max_completion_tokens` ≤ `cap` → no-op.
pub fn apply_openai_reasoning_budget(
    req: &mut ChatCompletionRequest,
    cap: u64,
) -> ReasoningOutcome {
    if !is_openai_reasoning_model(&req.model) {
        return ReasoningOutcome::NotApplicable;
    }
    match req.max_completion_tokens {
        None => {
            req.max_completion_tokens = Some(cap);
            ReasoningOutcome::Capped { cap }
        }
        Some(existing) if existing > cap => {
            req.max_completion_tokens = Some(cap);
            ReasoningOutcome::Capped { cap }
        }
        Some(existing) => ReasoningOutcome::AlreadyWithinBudget { existing },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn anthropic_req(model: &str) -> MessagesRequest {
        MessagesRequest {
            model: model.into(),
            system: json!(null),
            messages: vec![],
            stream: false,
            max_tokens: 4096,
            thinking: None,
        }
    }

    fn openai_req(model: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.into(),
            messages: vec![],
            stream: false,
            max_completion_tokens: None,
        }
    }

    // ── model detection ───────────────────────────────────────────────────────

    #[test]
    fn detects_claude_opus_4_8() {
        assert!(is_anthropic_reasoning_model("claude-opus-4-8"));
    }

    #[test]
    fn detects_claude_opus_4() {
        assert!(is_anthropic_reasoning_model("claude-opus-4"));
    }

    #[test]
    fn does_not_flag_sonnet() {
        assert!(!is_anthropic_reasoning_model("claude-sonnet-5"));
        assert!(!is_anthropic_reasoning_model("claude-haiku-4-5-20251001"));
    }

    #[test]
    fn detects_o3_family() {
        assert!(is_openai_reasoning_model("o3"));
        assert!(is_openai_reasoning_model("o3-mini"));
        assert!(is_openai_reasoning_model("o3-pro"));
    }

    #[test]
    fn detects_o1_family() {
        assert!(is_openai_reasoning_model("o1"));
        assert!(is_openai_reasoning_model("o1-mini"));
        assert!(is_openai_reasoning_model("o1-preview"));
    }

    #[test]
    fn does_not_flag_gpt4o() {
        assert!(!is_openai_reasoning_model("gpt-4o"));
        assert!(!is_openai_reasoning_model("gpt-4o-mini"));
        assert!(!is_openai_reasoning_model("gpt-4-turbo"));
    }

    // ── anthropic budget ──────────────────────────────────────────────────────

    #[test]
    fn anthropic_injects_thinking_when_absent() {
        let mut req = anthropic_req("claude-opus-4-8");
        let outcome = apply_anthropic_reasoning_budget(&mut req, 8_000);
        assert_eq!(outcome, ReasoningOutcome::Capped { cap: 8_000 });
        let t = req.thinking.unwrap();
        assert_eq!(t.budget_tokens, 8_000);
        assert_eq!(t.thinking_type, "enabled");
    }

    #[test]
    fn anthropic_reduces_excessive_budget() {
        let mut req = anthropic_req("claude-opus-4-8");
        req.thinking = Some(ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 50_000,
        });
        let outcome = apply_anthropic_reasoning_budget(&mut req, 8_000);
        assert_eq!(outcome, ReasoningOutcome::Capped { cap: 8_000 });
        assert_eq!(req.thinking.unwrap().budget_tokens, 8_000);
    }

    #[test]
    fn anthropic_leaves_conservative_budget_alone() {
        let mut req = anthropic_req("claude-opus-4-8");
        req.thinking = Some(ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 4_000,
        });
        let outcome = apply_anthropic_reasoning_budget(&mut req, 8_000);
        assert_eq!(
            outcome,
            ReasoningOutcome::AlreadyWithinBudget { existing: 4_000 }
        );
        assert_eq!(req.thinking.unwrap().budget_tokens, 4_000);
    }

    #[test]
    fn anthropic_no_op_for_non_reasoning_model() {
        let mut req = anthropic_req("claude-sonnet-5");
        let outcome = apply_anthropic_reasoning_budget(&mut req, 8_000);
        assert_eq!(outcome, ReasoningOutcome::NotApplicable);
        assert!(req.thinking.is_none());
    }

    // ── openai budget ─────────────────────────────────────────────────────────

    #[test]
    fn openai_injects_max_completion_tokens_when_absent() {
        let mut req = openai_req("o3");
        let outcome = apply_openai_reasoning_budget(&mut req, 16_000);
        assert_eq!(outcome, ReasoningOutcome::Capped { cap: 16_000 });
        assert_eq!(req.max_completion_tokens, Some(16_000));
    }

    #[test]
    fn openai_reduces_excessive_max_completion_tokens() {
        let mut req = openai_req("o3");
        req.max_completion_tokens = Some(100_000);
        let outcome = apply_openai_reasoning_budget(&mut req, 16_000);
        assert_eq!(outcome, ReasoningOutcome::Capped { cap: 16_000 });
        assert_eq!(req.max_completion_tokens, Some(16_000));
    }

    #[test]
    fn openai_leaves_conservative_max_completion_tokens_alone() {
        let mut req = openai_req("o3");
        req.max_completion_tokens = Some(8_000);
        let outcome = apply_openai_reasoning_budget(&mut req, 16_000);
        assert_eq!(
            outcome,
            ReasoningOutcome::AlreadyWithinBudget { existing: 8_000 }
        );
        assert_eq!(req.max_completion_tokens, Some(8_000));
    }

    #[test]
    fn openai_no_op_for_non_reasoning_model() {
        let mut req = openai_req("gpt-4o");
        let outcome = apply_openai_reasoning_budget(&mut req, 16_000);
        assert_eq!(outcome, ReasoningOutcome::NotApplicable);
        assert!(req.max_completion_tokens.is_none());
    }
}
