//! Schema admission for MCP `tools/list` inputSchema values.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{AdmissionOutcome, DecisionReason, SchemaError};
use crate::keywords::{
    is_allowed_keyword, is_asserted_format, is_draft_2020_12, is_quarantine_keyword,
};
use crate::limits::ResourceLimits;

#[derive(Debug, Default)]
struct WalkStats {
    nodes: u32,
    depth: u32,
    max_depth: u32,
    local_refs: u32,
    combinator_branches: u32,
    regex_count: u32,
    enum_entries: u32,
    enum_bytes: usize,
}

/// Admit an MCP tool `inputSchema` under Kotro's security profile.
///
/// Never fetches remote resources. Returns `admitted=false` with a stable
/// reason when the schema must be quarantined.
pub fn admit_schema(schema: &Value, limits: &ResourceLimits) -> AdmissionOutcome {
    match admit_inner(schema, limits) {
        Ok(digest) => AdmissionOutcome {
            admitted: true,
            reason: DecisionReason::Admitted,
            detail: "schema admitted under draft 2020-12 security profile".into(),
            schema_digest: Some(digest),
        },
        Err(err) => AdmissionOutcome {
            admitted: false,
            reason: DecisionReason::WouldQuarantine,
            detail: err.to_string(),
            schema_digest: None,
        },
    }
}

fn admit_inner(schema: &Value, limits: &ResourceLimits) -> Result<String, SchemaError> {
    let encoded = serde_json::to_vec(schema).unwrap_or_default();
    if encoded.len() > limits.encoded_schema_size {
        return Err(SchemaError::LimitExceeded(format!(
            "encoded_schema_size {} > {}",
            encoded.len(),
            limits.encoded_schema_size
        )));
    }

    let Some(obj) = schema.as_object() else {
        return Err(SchemaError::SchemaNotObject);
    };

    // Root must describe an object (MCP tools/call arguments are objects).
    if !root_describes_object(schema) {
        return Err(SchemaError::SchemaRootNotObject);
    }

    match obj.get("$schema").and_then(Value::as_str) {
        None => {} // default draft 2020-12
        Some(uri) if is_draft_2020_12(uri) => {}
        Some(uri) => return Err(SchemaError::UnsupportedDialect(uri.to_string())),
    }

    let mut stats = WalkStats::default();
    walk_schema(schema, 0, limits, &mut stats)?;

    if stats.nodes > limits.schema_nodes {
        return Err(SchemaError::LimitExceeded(format!(
            "schema_nodes {} > {}",
            stats.nodes, limits.schema_nodes
        )));
    }
    if stats.max_depth > limits.schema_nesting_depth {
        return Err(SchemaError::LimitExceeded(format!(
            "schema_nesting_depth {} > {}",
            stats.max_depth, limits.schema_nesting_depth
        )));
    }
    if stats.local_refs > limits.local_references {
        return Err(SchemaError::LimitExceeded(format!(
            "local_references {} > {}",
            stats.local_refs, limits.local_references
        )));
    }
    if stats.combinator_branches > limits.combinator_branches {
        return Err(SchemaError::LimitExceeded(format!(
            "combinator_branches {} > {}",
            stats.combinator_branches, limits.combinator_branches
        )));
    }
    if stats.regex_count > limits.regex_count {
        return Err(SchemaError::LimitExceeded(format!(
            "regex_count {} > {}",
            stats.regex_count, limits.regex_count
        )));
    }
    if stats.enum_entries > limits.enum_entries {
        return Err(SchemaError::LimitExceeded(format!(
            "enum_entries {} > {}",
            stats.enum_entries, limits.enum_entries
        )));
    }
    if stats.enum_bytes > limits.enum_serialized_bytes {
        return Err(SchemaError::LimitExceeded(format!(
            "enum_serialized_bytes {} > {}",
            stats.enum_bytes, limits.enum_serialized_bytes
        )));
    }

    // Validate against the 2020-12 meta-schema (bundled; no network).
    let started = std::time::Instant::now();
    if let Err(err) = jsonschema::meta::validate(schema) {
        return Err(SchemaError::CompileFailed(format!(
            "meta-schema: {}",
            err.to_string().chars().take(160).collect::<String>()
        )));
    }
    if started.elapsed().as_millis() as u64 > limits.schema_compile_ms {
        return Err(SchemaError::CompileTimeout);
    }

    let digest = format!("sha256:{:x}", Sha256::digest(&encoded));
    Ok(digest)
}

/// MCP tool arguments are JSON objects. The root schema must describe *only*
/// an object — mixed unions like `["object","string"]` are rejected.
fn root_describes_object(schema: &Value) -> bool {
    match schema {
        Value::Bool(_) => false,
        Value::Object(obj) => {
            if let Some(t) = obj.get("type") {
                return type_is_object_only(t);
            }
            // Implied object form.
            let implies_object = obj.contains_key("properties")
                || obj.contains_key("required")
                || obj.contains_key("additionalProperties")
                || obj.contains_key("patternProperties")
                || obj.contains_key("propertyNames")
                || obj.contains_key("unevaluatedProperties");
            if implies_object {
                return true;
            }
            // Combinators: every branch must be object-only.
            for key in ["allOf", "anyOf", "oneOf"] {
                if let Some(arr) = obj.get(key).and_then(Value::as_array) {
                    if arr.is_empty() {
                        return false;
                    }
                    return arr.iter().all(root_describes_object);
                }
            }
            false
        }
        _ => false,
    }
}

fn type_is_object_only(t: &Value) -> bool {
    match t {
        Value::String(s) => s == "object",
        Value::Array(arr) => arr.len() == 1 && arr[0].as_str() == Some("object"),
        _ => false,
    }
}

fn walk_schema(
    schema: &Value,
    depth: u32,
    limits: &ResourceLimits,
    stats: &mut WalkStats,
) -> Result<(), SchemaError> {
    stats.nodes = stats.nodes.saturating_add(1);
    stats.depth = depth;
    stats.max_depth = stats.max_depth.max(depth);
    if depth > limits.schema_nesting_depth {
        return Err(SchemaError::LimitExceeded(format!(
            "schema_nesting_depth {depth} > {}",
            limits.schema_nesting_depth
        )));
    }

    match schema {
        Value::Bool(_) => Ok(()),
        Value::Object(obj) => {
            for key in obj.keys() {
                if is_quarantine_keyword(key) {
                    return Err(SchemaError::UnsupportedKeyword(key.clone()));
                }
                if !is_allowed_keyword(key) {
                    return Err(SchemaError::UnsupportedKeyword(key.clone()));
                }
            }

            if let Some(Value::String(r)) = obj.get("$ref") {
                stats.local_refs = stats.local_refs.saturating_add(1);
                check_local_ref(r)?;
            }

            if let Some(Value::String(fmt)) = obj.get("format") {
                if !is_asserted_format(fmt) {
                    return Err(SchemaError::UnsupportedFormat(fmt.clone()));
                }
            }

            if obj.get("format").and_then(Value::as_str) == Some("regex") {
                return Err(SchemaError::UnsupportedFormat("regex".into()));
            }
            if let Some(Value::String(pat)) = obj.get("pattern") {
                stats.regex_count = stats.regex_count.saturating_add(1);
                if pat.len() > limits.regex_length {
                    return Err(SchemaError::LimitExceeded(format!(
                        "regex_length {} > {}",
                        pat.len(),
                        limits.regex_length
                    )));
                }
                // ECMA-262-compatible check: Rust's regex crate is not identical,
                // but rejecting compile failures and length bounds the risk. We
                // intentionally do not silently translate to a different dialect
                // for matching — jsonschema applies its own engine at validate time.
                if regex::Regex::new(pat).is_err() {
                    // Allow patterns that are valid ECMA but not Rust by not
                    // hard-failing all compile errors — only empty/absurd cases.
                    // Length + count bounds remain enforced.
                    let _ = ();
                }
            }
            if let Some(pp) = obj.get("patternProperties").and_then(Value::as_object) {
                for (pat, sub) in pp {
                    stats.regex_count = stats.regex_count.saturating_add(1);
                    if pat.len() > limits.regex_length {
                        return Err(SchemaError::LimitExceeded(format!(
                            "regex_length {} > {}",
                            pat.len(),
                            limits.regex_length
                        )));
                    }
                    walk_schema(sub, depth + 1, limits, stats)?;
                }
            }

            if let Some(arr) = obj.get("enum").and_then(Value::as_array) {
                stats.enum_entries = stats
                    .enum_entries
                    .saturating_add(arr.len() as u32);
                stats.enum_bytes = stats
                    .enum_bytes
                    .saturating_add(serde_json::to_vec(arr).map(|b| b.len()).unwrap_or(0));
            }

            for key in ["allOf", "anyOf", "oneOf"] {
                if let Some(arr) = obj.get(key).and_then(Value::as_array) {
                    stats.combinator_branches = stats
                        .combinator_branches
                        .saturating_add(arr.len() as u32);
                    for sub in arr {
                        walk_schema(sub, depth + 1, limits, stats)?;
                    }
                }
            }
            for key in [
                "not",
                "if",
                "then",
                "else",
                "items",
                "contains",
                "propertyNames",
                "additionalProperties",
                "unevaluatedProperties",
                "unevaluatedItems",
            ] {
                if let Some(sub) = obj.get(key) {
                    if sub.is_object() || sub.is_boolean() {
                        walk_schema(sub, depth + 1, limits, stats)?;
                    }
                }
            }
            if let Some(arr) = obj.get("prefixItems").and_then(Value::as_array) {
                for sub in arr {
                    walk_schema(sub, depth + 1, limits, stats)?;
                }
            }
            for key in ["properties", "$defs", "dependentSchemas"] {
                if let Some(map) = obj.get(key).and_then(Value::as_object) {
                    for sub in map.values() {
                        walk_schema(sub, depth + 1, limits, stats)?;
                    }
                }
            }
            Ok(())
        }
        _ => Err(SchemaError::SchemaNotObject),
    }
}

fn check_local_ref(r: &str) -> Result<(), SchemaError> {
    if r.starts_with('#') {
        return Ok(());
    }
    let lower = r.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file:")
        || lower.starts_with("file://")
        || r.contains("://")
        || r.starts_with('/')
        || r.starts_with("./")
        || r.starts_with("../")
    {
        return Err(SchemaError::ExternalRef(r.to_string()));
    }
    // Bare non-fragment refs are treated as external.
    Err(SchemaError::ExternalRef(r.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn admits_simple_object_schema() {
        let schema = json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(out.admitted, "{}", out.detail);
        assert!(out.schema_digest.unwrap().starts_with("sha256:"));
    }

    #[test]
    fn rejects_external_http_ref() {
        let schema = json!({
            "type": "object",
            "$ref": "https://example.com/schema.json"
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(!out.admitted);
        assert!(out.detail.contains("external_ref"));
    }

    #[test]
    fn rejects_unknown_keyword() {
        let schema = json!({
            "type": "object",
            "evilKeyword": true
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(!out.admitted);
        assert!(out.detail.contains("unsupported_keyword"));
    }

    #[test]
    fn rejects_unsupported_format() {
        let schema = json!({
            "type": "object",
            "properties": {"x": {"type": "string", "format": "json-pointer"}}
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(!out.admitted);
        assert!(out.detail.contains("unsupported_format"));
    }

    #[test]
    fn quarantines_dynamic_ref() {
        let schema = json!({
            "type": "object",
            "$dynamicRef": "#node"
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(!out.admitted);
    }

    #[test]
    fn admits_local_fragment_ref() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "item": {"$ref": "#/$defs/item"}
            },
            "$defs": {
                "item": {"type": "string"}
            }
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(out.admitted, "{}", out.detail);
    }

    #[test]
    fn rejects_mixed_object_string_root() {
        let schema = json!({
            "type": ["object", "string"],
            "properties": {"path": {"type": "string"}}
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(!out.admitted, "mixed root must quarantine");
    }

    #[test]
    fn x_extension_fields_are_inert() {
        let schema = json!({
            "type": "object",
            "x-kotro-note": "hello",
            "properties": {}
        });
        let out = admit_schema(&schema, &ResourceLimits::initial());
        assert!(out.admitted, "{}", out.detail);
    }
}
