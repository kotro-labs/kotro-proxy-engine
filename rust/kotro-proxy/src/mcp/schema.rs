//! MCP tool argument validation — bounded JSON Schema 2020-12 (C5).
//!
//! The previous handwritten subset validator lived here. It is replaced by
//! `kotro-schema`, which admits schemas under a security profile and validates
//! arguments with draft 2020-12 semantics. The subset implementation is kept
//! only as `legacy_subset` for differential tests.

pub use kotro_schema::{
    admit_schema, apply_mode, args_hash, compile, parse_arguments, short_args_hash, validate,
    AdmittedSchema, AdmissionOutcome, DecisionReason, ResourceLimits, SanitizedError, SchemaError,
    ValidationResult,
};

#[cfg(test)]
mod legacy_subset {
    //! Frozen copy of the pre-C5 subset validator for behavioural comparison.
    use serde_json::Value;

    pub fn validate(args: &Value, schema: &Value) -> Vec<String> {
        let mut errors = Vec::new();
        validate_node(args, schema, "$", &mut errors);
        errors
    }

    fn validate_node(value: &Value, schema: &Value, path: &str, errors: &mut Vec<String>) {
        let Some(schema_obj) = schema.as_object() else {
            return;
        };
        if let Some(expected) = schema_obj.get("type").and_then(Value::as_str) {
            let ok = match (expected, value) {
                ("object", Value::Object(_))
                | ("array", Value::Array(_))
                | ("string", Value::String(_))
                | ("number", Value::Number(_))
                | ("boolean", Value::Bool(_))
                | ("null", Value::Null) => true,
                ("integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
                _ => false,
            };
            if !ok {
                errors.push(format!("{path}: expected {expected}"));
                return;
            }
        }
        if let Some(allowed) = schema_obj.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                errors.push(format!("{path}: value not in enum"));
            }
        }
        if let Some(obj) = value.as_object() {
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
        if let Some(arr) = value.as_array() {
            if let Some(item_schema) = schema_obj.get("items") {
                for (i, item) in arr.iter().enumerate() {
                    validate_node(item, item_schema, &format!("{path}[{i}]"), errors);
                }
            }
        }
    }

    #[test]
    fn subset_cases_still_fail_under_c5() {
        use serde_json::json;
        let schema = json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"},
                "mode": {"type": "string", "enum": ["read", "write"]}
            },
            "additionalProperties": false
        });
        assert!(super::validate(&json!({"path": "/tmp", "mode": "read"}), &schema).is_empty());
        assert!(!super::validate(&json!({"recursive": true}), &schema).is_empty());
        assert!(!super::validate(&json!({"path": 42}), &schema).is_empty());
        assert!(!super::validate(&json!({"path": "/x", "mode": "exec"}), &schema).is_empty());
        assert!(!super::validate(&json!({"path": "/x", "sneaky": "y"}), &schema).is_empty());
        let _ = validate(&json!({"path": "/x"}), &schema); // legacy still callable
    }
}
