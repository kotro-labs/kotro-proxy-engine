//! Minimal, deterministic JSON Schema validation for MCP tool arguments.
//!
//! Covers the subset MCP servers actually use for `inputSchema`: `type`,
//! `required`, `properties`, `items`, `enum`, and `additionalProperties: false`.
//! Unknown keywords are ignored (permissive), but declared constraints are
//! enforced strictly — malformed arguments never reach the tool.

use serde_json::Value;

/// Validate `args` against `schema`. Returns a list of violations (empty = valid).
pub fn validate(args: &Value, schema: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    validate_node(args, schema, "$", &mut errors);
    errors
}

fn validate_node(value: &Value, schema: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(schema_obj) = schema.as_object() else {
        return; // non-object schema — nothing to enforce
    };

    if let Some(expected) = schema_obj.get("type").and_then(Value::as_str) {
        if !type_matches(value, expected) {
            errors.push(format!(
                "{path}: expected {expected}, got {}",
                type_name(value)
            ));
            return; // deeper checks are meaningless on the wrong type
        }
    }

    if let Some(allowed) = schema_obj.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            errors.push(format!("{path}: value not in enum"));
        }
    }

    if value.is_object() {
        let obj = value.as_object().unwrap();
        if let Some(required) = schema_obj.get("required").and_then(Value::as_array) {
            for req in required.iter().filter_map(Value::as_str) {
                if !obj.contains_key(req) {
                    errors.push(format!("{path}: missing required property '{req}'"));
                }
            }
        }
        let properties = schema_obj.get("properties").and_then(Value::as_object);
        if let Some(props) = properties {
            for (key, sub_schema) in props {
                if let Some(sub_value) = obj.get(key) {
                    validate_node(sub_value, sub_schema, &format!("{path}.{key}"), errors);
                }
            }
        }
        if schema_obj.get("additionalProperties") == Some(&Value::Bool(false)) {
            for key in obj.keys() {
                if properties.map(|p| !p.contains_key(key)).unwrap_or(true) {
                    errors.push(format!("{path}: unexpected property '{key}'"));
                }
            }
        }
    }

    if value.is_array() {
        if let Some(item_schema) = schema_obj.get("items") {
            for (i, item) in value.as_array().unwrap().iter().enumerate() {
                validate_node(item, item_schema, &format!("{path}[{i}]"), errors);
            }
        }
    }
}

fn type_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file_schema() -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"},
                "recursive": {"type": "boolean"},
                "mode": {"type": "string", "enum": ["read", "write"]}
            },
            "additionalProperties": false
        })
    }

    #[test]
    fn valid_args_pass() {
        assert!(validate(&json!({"path": "/tmp/x", "recursive": true}), &file_schema()).is_empty());
    }

    #[test]
    fn missing_required_fails() {
        let errs = validate(&json!({"recursive": true}), &file_schema());
        assert!(errs.iter().any(|e| e.contains("missing required property 'path'")));
    }

    #[test]
    fn wrong_type_fails() {
        let errs = validate(&json!({"path": 42}), &file_schema());
        assert!(errs.iter().any(|e| e.contains("expected string")));
    }

    #[test]
    fn enum_violation_fails() {
        let errs = validate(&json!({"path": "/x", "mode": "exec"}), &file_schema());
        assert!(errs.iter().any(|e| e.contains("not in enum")));
    }

    #[test]
    fn additional_property_rejected() {
        let errs = validate(&json!({"path": "/x", "sneaky": "y"}), &file_schema());
        assert!(errs.iter().any(|e| e.contains("unexpected property 'sneaky'")));
    }

    #[test]
    fn nested_arrays_validated() {
        let schema = json!({
            "type": "object",
            "properties": {"files": {"type": "array", "items": {"type": "string"}}}
        });
        let errs = validate(&json!({"files": ["a", 3]}), &schema);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("files[1]"));
    }
}
