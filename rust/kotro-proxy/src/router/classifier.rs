//! Prompt complexity classification and model routing.
//!
//! Classifies incoming requests into four tiers so the proxy can route to the
//! cheapest model that can still handle the request — cutting costs on the 60–70%
//! of agent turns that don't need a frontier model.
//!
//! ## Tiers
//!
//! | Tier | Description | Default routing |
//! |------|-------------|-----------------|
//! | `Nano` | Trivial edits (lint, format, fix typo) | `KOTRO_LOCAL_UPSTREAM_URL` + `KOTRO_MOE_DEFAULT_MODEL` |
//! | `Micro` | Short Q&A, single-step, no code | `KOTRO_CHEAP_MODEL` (if set), otherwise default |
//! | `Standard` | Multi-turn coding, standard requests | Configured model (unchanged) |
//! | `Complex` | Long context, reasoning, tool-heavy | Configured model (logged; expensive model support planned) |
//!
//! ## Classification signals
//!
//! | Signal | Effect |
//! |--------|--------|
//! | text < 150 chars + trivial keyword | → Nano |
//! | text < 600 chars, no code, no reasoning phrase | → Micro |
//! | text > 3000 chars OR reasoning phrase OR deep tool history | → Complex |
//! | else | → Standard |
//!
//! ## Configuration
//!
//! | Env var | Default | Description |
//! |---------|---------|-------------|
//! | `KOTRO_CHEAP_MODEL` | (none) | Model name for `Micro` tier |
//! | `KOTRO_CHEAP_MODEL_URL` | (none) | Upstream base URL for cheap model (omit = same upstream) |
//!
//! When `KOTRO_CHEAP_MODEL` is not set, `Micro` falls back to `Standard`.

use std::sync::OnceLock;

use regex::Regex;

// ── Compiled regexes (initialized once) ──────────────────────────────────────

static TRIVIAL_RE: OnceLock<Regex> = OnceLock::new();
static REASONING_RE: OnceLock<Regex> = OnceLock::new();
static CODE_BLOCK_RE: OnceLock<Regex> = OnceLock::new();
static INLINE_CODE_RE: OnceLock<Regex> = OnceLock::new();

fn trivial_re() -> &'static Regex {
    TRIVIAL_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(fix\s+typo|lint|format|cleanup|add\s+comment|json|syntax\s+error)\b",
        )
        .expect("trivial_re")
    })
}

fn reasoning_re() -> &'static Regex {
    REASONING_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(step[\s-]by[\s-]step|think\s+(through|about|carefully)|reason\s+(through|about)|analyze\s+in\s+detail|break\s+(it\s+)?down|deep\s+dive|comprehensive\s+(analysis|review|plan)|architect|design\s+pattern|trade[\s-]off|pros\s+and\s+cons)\b",
        )
        .expect("reasoning_re")
    })
}

fn code_block_re() -> &'static Regex {
    CODE_BLOCK_RE.get_or_init(|| Regex::new(r"```[\s\S]*?```").expect("code_block_re"))
}

fn inline_code_re() -> &'static Regex {
    INLINE_CODE_RE.get_or_init(|| Regex::new(r"`[^`]+`").expect("inline_code_re"))
}

// ── Public types ──────────────────────────────────────────────────────────────

/// Prompt complexity tier, used by the model router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptComplexity {
    /// Trivial one-liner (lint, format, typo). Route to fastest local model.
    Nano,
    /// Short Q&A or simple single-step task. Route to cheapest API model.
    Micro,
    /// Standard multi-turn coding or moderate instruction following.
    Standard,
    /// Long context, reasoning-heavy, or tool-rich agent workflow.
    Complex,
}

// ── Classification ────────────────────────────────────────────────────────────

/// Classify a prompt into a [`PromptComplexity`] tier.
///
/// # Parameters
///
/// - `prompt`: The latest user turn (or system prompt + latest user turn).
/// - `total_messages`: Total message count in the conversation so far
///   (used to detect deep agent workflows).
/// - `has_active_tool_calls`: `true` when the conversation contains tool-call
///   / tool-result turns (indicates an agent workflow → at least Standard).
pub fn classify_complexity(
    prompt: &str,
    total_messages: usize,
    has_active_tool_calls: bool,
) -> PromptComplexity {
    // Strip fenced code blocks before measuring text length so a 5-word
    // question with a large code attachment isn't downgraded to Nano/Micro.
    let stripped_fenced = code_block_re().replace_all(prompt, "");
    let stripped = inline_code_re().replace_all(&stripped_fenced, "");
    let text = stripped.trim();
    let text_len = text.len();

    // ── Complex signals ───────────────────────────────────────────────────────
    // Check these first because they can up-classify any prompt regardless of
    // length.
    let has_reasoning = reasoning_re().is_match(text);

    // Deep tool history: 10+ messages likely means multi-step agent workflow.
    let deep_tool_history = has_active_tool_calls && total_messages >= 10;

    if has_reasoning || deep_tool_history || text_len > 3000 {
        return PromptComplexity::Complex;
    }

    // ── Nano ──────────────────────────────────────────────────────────────────
    if text_len < 150 && trivial_re().is_match(text) {
        return PromptComplexity::Nano;
    }

    // ── Micro ─────────────────────────────────────────────────────────────────
    // Short, no code, no active tool calls, no reasoning signals.
    let original_has_code =
        code_block_re().is_match(prompt) || inline_code_re().is_match(prompt);

    if text_len < 600 && !original_has_code && !has_active_tool_calls {
        return PromptComplexity::Micro;
    }

    PromptComplexity::Standard
}

// ── Legacy helper (kept for call sites not yet migrated to classify_complexity) ─

/// Returns `true` if the prompt is trivial enough to bypass cloud models.
///
/// Deprecated in favour of [`classify_complexity`] returning [`PromptComplexity::Nano`].
pub fn is_trivial_prompt(prompt: &str) -> bool {
    classify_complexity(prompt, 1, false) == PromptComplexity::Nano
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(prompt: &str) -> PromptComplexity {
        classify_complexity(prompt, 1, false)
    }

    // ── Nano ─────────────────────────────────────────────────────────────────

    #[test]
    fn nano_fix_typo() {
        assert_eq!(classify("fix typo in the print statement"), PromptComplexity::Nano);
    }

    #[test]
    fn nano_format_json() {
        assert_eq!(classify("format this file as json"), PromptComplexity::Nano);
    }

    #[test]
    fn nano_cleanup() {
        assert_eq!(classify("cleanup the whitespace here"), PromptComplexity::Nano);
    }

    #[test]
    fn nano_lint() {
        assert_eq!(classify("please lint this code"), PromptComplexity::Nano);
    }

    #[test]
    fn nano_with_code_block_still_nano() {
        // The code block is stripped before measuring text; the remaining text
        // is "fix typo" → Nano.
        assert_eq!(
            classify("fix typo\n```rust\nfn main() {}\n```"),
            PromptComplexity::Nano
        );
    }

    // ── Micro ─────────────────────────────────────────────────────────────────

    #[test]
    fn micro_short_question_no_code() {
        assert_eq!(
            classify("What does the map() function do in Rust?"),
            PromptComplexity::Micro
        );
    }

    #[test]
    fn micro_short_chat() {
        assert_eq!(
            classify("Can you summarize what a hash map is in one sentence?"),
            PromptComplexity::Micro
        );
    }

    // ── Standard ──────────────────────────────────────────────────────────────

    #[test]
    fn standard_with_code_block() {
        let prompt = "Here's my function:\n```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```\nWhat does it do?";
        assert_eq!(classify(prompt), PromptComplexity::Standard);
    }

    #[test]
    fn standard_with_inline_code() {
        // Inline backtick code counts as code → Standard, not Micro
        let prompt = "Explain how `Arc<Mutex<T>>` differs from `Rc<RefCell<T>>` in Rust, and when you would choose each.";
        assert_eq!(classify(prompt), PromptComplexity::Standard);
    }

    #[test]
    fn standard_long_question_no_reasoning() {
        // >600 chars, no code, no reasoning phrase → Standard (length alone exceeds Micro threshold)
        let prompt = "I need to understand how the Rust borrow checker works with mutable references. \
            Can you walk me through what happens when you have two mutable borrows active at the \
            same time, and why the compiler rejects this pattern? I have a vector of structs where \
            I want to mutably borrow one element while reading another element in the same loop \
            iteration. The compiler says I cannot have both borrows active simultaneously. Is there \
            a standard pattern for working around this? Should I use indices instead of iterators, \
            or is there a way to convince the borrow checker that these borrows do not alias? I \
            have tried splitting the slice but the ergonomics are awkward for my use case.";
        assert!(prompt.len() > 600, "test prompt must be >600 chars to exceed Micro threshold");
        assert_eq!(classify(prompt), PromptComplexity::Standard);
    }

    #[test]
    fn standard_not_nano_when_too_long() {
        let long_prompt = "I need you to fix a typo, but also redesign the entire backend architecture so that it can scale to millions of users. Please implement the caching layer using Redis and add unit tests.";
        assert_ne!(classify(long_prompt), PromptComplexity::Nano);
    }

    // ── Complex ───────────────────────────────────────────────────────────────

    #[test]
    fn complex_step_by_step() {
        let prompt = "Walk me step-by-step through how to implement a distributed lock using Redis.";
        assert_eq!(classify(prompt), PromptComplexity::Complex);
    }

    #[test]
    fn complex_think_through() {
        let prompt = "Think through the trade-offs between using a message queue vs. direct REST calls.";
        assert_eq!(classify(prompt), PromptComplexity::Complex);
    }

    #[test]
    fn complex_analyze_in_detail() {
        let prompt = "Analyze in detail how the TCP handshake works and why SYN flooding is effective.";
        assert_eq!(classify(prompt), PromptComplexity::Complex);
    }

    #[test]
    fn complex_long_prompt() {
        // > 3000 chars → Complex regardless of content
        let long = "x".repeat(3001);
        assert_eq!(classify(&long), PromptComplexity::Complex);
    }

    #[test]
    fn complex_deep_tool_history() {
        let prompt = "What files are left to check?";
        // 15 messages + active tool calls → deep agent workflow → Complex
        assert_eq!(
            classify_complexity(prompt, 15, true),
            PromptComplexity::Complex
        );
    }

    // ── Tool-call handling ────────────────────────────────────────────────────

    #[test]
    fn active_tool_calls_prevents_micro() {
        // Would otherwise be Micro (short, no code) but tool calls make it
        // at least Standard.
        let prompt = "ok what about the other file?";
        assert_ne!(
            classify_complexity(prompt, 4, true),
            PromptComplexity::Micro
        );
    }

    // ── Legacy helper ─────────────────────────────────────────────────────────

    #[test]
    fn is_trivial_prompt_still_works() {
        assert!(is_trivial_prompt("fix typo in the print statement"));
        assert!(!is_trivial_prompt("Explain microservice architecture."));
    }
}
