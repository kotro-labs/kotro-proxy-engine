//! Hybrid Shrink Mode - Text minification algorithms.

use regex::Regex;
use std::sync::OnceLock;

static JSON_BLOCK_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_json_regex() -> &'static Regex {
    JSON_BLOCK_REGEX.get_or_init(|| {
        // Matches ```json\n{...}\n``` non-greedily
        Regex::new(r"```json\s*\n([\s\S]*?)\n```").unwrap()
    })
}

/// Shrinks text by removing trailing whitespace, collapsing blank lines,
/// and minifying any JSON markdown blocks found within.
pub fn shrink_text(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }

    let mut out = String::with_capacity(content.len());
    let mut consecutive_newlines = 0;

    for line in content.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            consecutive_newlines += 1;
            if consecutive_newlines <= 1 {
                out.push('\n');
            }
        } else {
            if consecutive_newlines > 0 && !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(trimmed);
            out.push('\n');
            consecutive_newlines = 0;
        }
    }
    
    // Remove the final trailing newline if it wasn't there originally
    if out.ends_with('\n') && !content.ends_with('\n') {
        out.pop();
    }

    // Shrink JSON blocks
    let re = get_json_regex();
    let shrunk_json = re.replace_all(&out, |caps: &regex::Captures| {
        let json_str = caps.get(1).map_or("", |m| m.as_str());
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            // Re-serialize with NO pretty-printing to crush it into one line
            if let Ok(minified) = serde_json::to_string(&parsed) {
                return format!("```json\n{}\n```", minified);
            }
        }
        caps[0].to_string()
    });

    shrunk_json.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shrink_trailing_whitespace() {
        let input = "hello  \nworld \t\n";
        assert_eq!(shrink_text(input), "hello\nworld\n");
    }

    #[test]
    fn test_shrink_blank_lines() {
        let input = "line1\n\n\n\nline2";
        assert_eq!(shrink_text(input), "line1\n\nline2");
    }

    #[test]
    fn test_shrink_json_blocks() {
        let input = "Here is some json:\n```json\n{\n  \"key\": \"value\",\n  \"list\": [\n    1,\n    2\n  ]\n}\n```\nDone.";
        let expected = "Here is some json:\n```json\n{\"key\":\"value\",\"list\":[1,2]}\n```\nDone.";
        assert_eq!(shrink_text(input), expected);
    }
}
