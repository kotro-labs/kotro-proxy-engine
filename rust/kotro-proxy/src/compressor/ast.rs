use tree_sitter::{Node, Parser};

/// Prune comments and docstrings from a Rust or Python code block.
/// Unrecognized languages are returned unchanged.
pub fn prune_code_block(lang: &str, code: &str) -> String {
    if code.len() > 500 * 1024 {
        return code.to_string(); // Skip AST parsing for massive files to prevent CPU locking
    }

    let language = match lang {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        _ => return code.to_string(), // unsupported language
    };

    let mut parser = Parser::new();
    if let Err(e) = parser.set_language(&language) {
        println!("set_language error: {}", e);
        return code.to_string();
    }
    
    let tree = match parser.parse(code, None) {
        Some(t) => t,
        None => {
            println!("parse returned None");
            return code.to_string();
        }
    };

    let mut spans_to_remove = Vec::new();
    find_pruneable_nodes(tree.root_node(), lang, &mut spans_to_remove);

    if spans_to_remove.is_empty() {
        return code.to_string();
    }

    // Sort spans by start_byte
    spans_to_remove.sort_by_key(|s| s.0);

    // Merge overlapping spans
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for span in spans_to_remove {
        if let Some(last) = merged.last_mut() {
            if span.0 <= last.1 {
                last.1 = last.1.max(span.1);
                continue;
            }
        }
        merged.push(span);
    }

    let mut result = String::with_capacity(code.len());
    let mut last_end = 0;

    for (start, end) in merged {
        if start > last_end {
            result.push_str(&code[last_end..start]);
        }
        last_end = end;
    }
    if last_end < code.len() {
        result.push_str(&code[last_end..]);
    }

    result
}

fn find_pruneable_nodes<'a>(node: Node<'a>, lang: &str, spans: &mut Vec<(usize, usize)>) {
    let kind = node.kind();
    
    let is_comment = match lang {
        "rust" => kind == "line_comment" || kind == "block_comment",
        "python" => kind == "comment",
        _ => false,
    };

    if is_comment {
        spans.push((node.start_byte(), node.end_byte()));
        return; // Don't traverse inside comments
    }

    // Python Docstrings
    // A docstring is an expression_statement containing a string, at the start of a block/module
    if lang == "python" && kind == "expression_statement" {
        if let Some(child) = node.child(0) {
            if child.kind() == "string" {
                if let Some(parent) = node.parent() {
                    let parent_kind = parent.kind();
                    if parent_kind == "block" || parent_kind == "module" {
                        // Check if it's the very first node in this block/module
                        if let Some(first_child) = parent.child(0) {
                            if first_child.id() == node.id() {
                                spans.push((node.start_byte(), node.end_byte()));
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_pruneable_nodes(child, lang, spans);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prune_rust_comments() {
        let code = r#"
// License header
// copyright 2026

fn main() {
    /* block comment */
    let x = 5; // inline comment
}
"#;
        let pruned = prune_code_block("rust", code);
        // Note: It might leave blank lines, but `shrink_text` handles that.
        assert!(!pruned.contains("License header"));
        assert!(!pruned.contains("copyright"));
        assert!(!pruned.contains("block comment"));
        assert!(!pruned.contains("inline comment"));
        assert!(pruned.contains("fn main() {"));
        assert!(pruned.contains("let x = 5;"));
    }

    #[test]
    fn test_prune_python_docstrings() {
        let code = r#"
"""
Module level docstring
"""

def my_func():
    """
    Function docstring
    """
    # Just a comment
    return 42
"#;
        let pruned = prune_code_block("python", code);
        assert!(!pruned.contains("Module level docstring"));
        assert!(!pruned.contains("Function docstring"));
        assert!(!pruned.contains("Just a comment"));
        assert!(pruned.contains("def my_func():"));
        assert!(pruned.contains("return 42"));
    }
}
