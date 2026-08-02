//! Compile admitted schemas and validate tool-call arguments.

use std::sync::Arc;
use std::time::Instant;

use jsonschema::Draft;
use serde_json::Value;

use crate::admit::admit_schema;
use crate::error::{DecisionReason, SanitizedError, SchemaError};
use crate::jcs;
use crate::limits::ResourceLimits;

/// A compiled, admitted schema ready for argument validation.
#[derive(Clone)]
pub struct AdmittedSchema {
    pub schema: Value,
    pub digest: String,
    validator: Arc<jsonschema::Validator>,
    limits: ResourceLimits,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub reason: DecisionReason,
    pub errors: Vec<SanitizedError>,
    pub args_hash: Option<String>,
    pub short_args_hash: Option<String>,
    pub detail: String,
}

/// Compile a schema after admission. Fails closed on unsupported constructs.
pub fn compile(schema: &Value, limits: &ResourceLimits) -> Result<AdmittedSchema, SchemaError> {
    let limits = limits.clone().clamp();
    let admission = admit_schema(schema, &limits);
    if !admission.admitted {
        return Err(SchemaError::CompileFailed(admission.detail));
    }
    let digest = admission.schema_digest.unwrap();

    let started = Instant::now();
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .should_ignore_unknown_formats(false)
        .build(schema)
        .map_err(|e| SchemaError::CompileFailed(sanitize_compile_error(&e.to_string())))?;

    if started.elapsed().as_millis() as u64 > limits.schema_compile_ms {
        return Err(SchemaError::CompileTimeout);
    }

    Ok(AdmittedSchema {
        schema: schema.clone(),
        digest,
        validator: Arc::new(validator),
        limits,
    })
}

impl AdmittedSchema {
    /// Validate already-parsed arguments (duplicate keys must be rejected earlier).
    pub fn validate_value(&self, args: &Value) -> ValidationResult {
        // Encoded size bound before any schema evaluation (also covers MCP
        // callers that already hold a `Value`).
        let encoded_len = serde_json::to_vec(args).map(|b| b.len()).unwrap_or(usize::MAX);
        if encoded_len > self.limits.encoded_arguments_size {
            return ValidationResult {
                ok: false,
                reason: DecisionReason::InvalidParameters,
                errors: vec![SanitizedError::new("$", "limit", "arguments_oversized")],
                args_hash: None,
                short_args_hash: None,
                detail: "arguments_oversized".into(),
            };
        }
        if let Err(e) = check_argument_structure(args, &self.limits) {
            return ValidationResult {
                ok: false,
                reason: DecisionReason::InvalidParameters,
                errors: vec![SanitizedError::new("$", "limit", e.to_string())],
                args_hash: None,
                short_args_hash: None,
                detail: e.to_string(),
            };
        }

        let deadline = std::time::Duration::from_millis(self.limits.validation_deadline_ms);
        let max_errors = self.limits.validation_errors_retained;
        let validator = Arc::clone(&self.validator);
        let args_cloned = args.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut errors = Vec::new();
            for err in validator.iter_errors(&args_cloned) {
                if errors.len() as u32 >= max_errors {
                    break;
                }
                errors.push(sanitize_jsonschema_error(&err));
            }
            let _ = tx.send(errors);
        });
        let errors = match rx.recv_timeout(deadline) {
            Ok(errors) => errors,
            Err(_) => {
                return ValidationResult {
                    ok: false,
                    reason: DecisionReason::ValidationUnavailable,
                    errors: vec![],
                    args_hash: None,
                    short_args_hash: None,
                    detail: "validation_unavailable".into(),
                };
            }
        };

        if !errors.is_empty() {
            return ValidationResult {
                ok: false,
                reason: DecisionReason::WouldDeny,
                errors,
                args_hash: None,
                short_args_hash: None,
                detail: "invalid_parameters".into(),
            };
        }

        let args_hash = match jcs::args_hash(args) {
            Ok(h) => h,
            Err(e) => {
                return ValidationResult {
                    ok: false,
                    reason: DecisionReason::ValidationUnavailable,
                    errors: vec![],
                    args_hash: None,
                    short_args_hash: None,
                    detail: format!("jcs_failed:{e}"),
                };
            }
        };
        let short_args_hash = jcs::short_args_hash(args).ok();

        ValidationResult {
            ok: true,
            reason: DecisionReason::Valid,
            errors: vec![],
            args_hash: Some(args_hash),
            short_args_hash,
            detail: "valid".into(),
        }
    }
}

/// Parse any JSON document, rejecting duplicate object keys.
pub fn parse_json_rejecting_duplicates(raw: &[u8]) -> Result<Value, SchemaError> {
    reject_duplicate_keys(raw)?;
    serde_json::from_slice(raw).map_err(|e| SchemaError::MalformedJson(e.to_string()))
}

/// Parse JSON arguments, rejecting duplicate object keys and oversized payloads
/// before schema evaluation.
pub fn parse_arguments(raw: &[u8], limits: &ResourceLimits) -> Result<Value, SchemaError> {
    if raw.len() > limits.encoded_arguments_size {
        return Err(SchemaError::ArgumentsOversized);
    }
    reject_duplicate_keys(raw)?;
    let value: Value = serde_json::from_slice(raw)
        .map_err(|e| SchemaError::MalformedJson(e.to_string()))?;
    check_argument_structure(&value, limits)?;
    Ok(value)
}

fn check_argument_structure(value: &Value, limits: &ResourceLimits) -> Result<(), SchemaError> {
    let mut nodes = 0u32;
    walk_args(value, 0, limits, &mut nodes)?;
    if nodes > limits.argument_nodes {
        return Err(SchemaError::LimitExceeded(format!(
            "argument_nodes {nodes} > {}",
            limits.argument_nodes
        )));
    }
    Ok(())
}

fn walk_args(
    value: &Value,
    depth: u32,
    limits: &ResourceLimits,
    nodes: &mut u32,
) -> Result<(), SchemaError> {
    *nodes = nodes.saturating_add(1);
    if depth > limits.argument_nesting_depth {
        return Err(SchemaError::LimitExceeded(format!(
            "argument_nesting_depth {depth} > {}",
            limits.argument_nesting_depth
        )));
    }
    match value {
        Value::String(s) if s.len() > limits.individual_string => {
            Err(SchemaError::LimitExceeded(format!(
                "individual_string {} > {}",
                s.len(),
                limits.individual_string
            )))
        }
        Value::Array(arr) => {
            if arr.len() as u32 > limits.array_elements {
                return Err(SchemaError::LimitExceeded(format!(
                    "array_elements {} > {}",
                    arr.len(),
                    limits.array_elements
                )));
            }
            for item in arr {
                walk_args(item, depth + 1, limits, nodes)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            if map.len() as u32 > limits.object_properties {
                return Err(SchemaError::LimitExceeded(format!(
                    "object_properties {} > {}",
                    map.len(),
                    limits.object_properties
                )));
            }
            for v in map.values() {
                walk_args(v, depth + 1, limits, nodes)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Scan raw JSON text for duplicate keys at every object depth.
/// This is a structural scan; it does not evaluate escape sequences inside strings
/// beyond basic JSON string skipping.
fn reject_duplicate_keys(raw: &[u8]) -> Result<(), SchemaError> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| SchemaError::MalformedJson("utf-8".into()))?;
    let mut chars = text.chars().peekable();
    scan_value(&mut chars, &mut Vec::new())
}

fn scan_value(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    _stack: &mut Vec<()>,
) -> Result<(), SchemaError> {
    skip_ws(chars);
    match chars.peek().copied() {
        Some('{') => scan_object(chars),
        Some('[') => scan_array(chars),
        Some('"') => {
            skip_string(chars)?;
            Ok(())
        }
        Some('t') => consume_lit(chars, "true"),
        Some('f') => consume_lit(chars, "false"),
        Some('n') => consume_lit(chars, "null"),
        Some('-') | Some('0'..='9') => {
            while matches!(chars.peek(), Some(c) if "0123456789-+eE.".contains(*c)) {
                chars.next();
            }
            Ok(())
        }
        _ => Err(SchemaError::MalformedJson("unexpected token".into())),
    }
}

fn scan_object(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(), SchemaError> {
    chars.next(); // '{'
    skip_ws(chars);
    let mut seen = std::collections::HashSet::new();
    if chars.peek() == Some(&'}') {
        chars.next();
        return Ok(());
    }
    loop {
        skip_ws(chars);
        let key = read_string(chars)?;
        if !seen.insert(key.clone()) {
            return Err(SchemaError::DuplicateObjectKey(key));
        }
        skip_ws(chars);
        if chars.next() != Some(':') {
            return Err(SchemaError::MalformedJson("expected ':'".into()));
        }
        scan_value(chars, &mut Vec::new())?;
        skip_ws(chars);
        match chars.next() {
            Some(',') => continue,
            Some('}') => return Ok(()),
            _ => return Err(SchemaError::MalformedJson("object".into())),
        }
    }
}

fn scan_array(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(), SchemaError> {
    chars.next(); // '['
    skip_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Ok(());
    }
    loop {
        scan_value(chars, &mut Vec::new())?;
        skip_ws(chars);
        match chars.next() {
            Some(',') => continue,
            Some(']') => return Ok(()),
            _ => return Err(SchemaError::MalformedJson("array".into())),
        }
    }
}

fn skip_ws(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}

fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<(), SchemaError> {
    let _ = read_string(chars)?;
    Ok(())
}

fn read_string(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, SchemaError> {
    if chars.next() != Some('"') {
        return Err(SchemaError::MalformedJson("string".into()));
    }
    let mut out = String::new();
    loop {
        match chars.next() {
            Some('\\') => {
                let esc = chars
                    .next()
                    .ok_or_else(|| SchemaError::MalformedJson("escape".into()))?;
                out.push('\\');
                out.push(esc);
                if esc == 'u' {
                    for _ in 0..4 {
                        let c = chars
                            .next()
                            .ok_or_else(|| SchemaError::MalformedJson("unicode".into()))?;
                        out.push(c);
                    }
                }
            }
            Some('"') => return Ok(out),
            Some(c) => out.push(c),
            None => return Err(SchemaError::MalformedJson("unterminated string".into())),
        }
    }
}

fn consume_lit(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    lit: &str,
) -> Result<(), SchemaError> {
    for expected in lit.chars() {
        if chars.next() != Some(expected) {
            return Err(SchemaError::MalformedJson(lit.into()));
        }
    }
    Ok(())
}

fn sanitize_compile_error(msg: &str) -> String {
    // Drop any accidental instance snippets; keep keyword-ish text only.
    let trimmed = if msg.len() > 200 { &msg[..200] } else { msg };
    trimmed
        .chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { ' ' })
        .collect()
}

fn sanitize_jsonschema_error(err: &jsonschema::ValidationError<'_>) -> SanitizedError {
    let path = err.instance_path().to_string();
    let path = if path.is_empty() { "$".into() } else { format!("${path}") };
    let keyword = err
        .schema_path()
        .to_string()
        .rsplit('/')
        .next()
        .unwrap_or("schema")
        .to_string();
    // Never include the instance value — only a generic message + keyword.
    SanitizedError::new(path, keyword, "constraint failed")
}

/// Apply audit vs enforce to an admission/validation failure.
pub fn apply_mode(enforces: bool, reason: DecisionReason) -> DecisionReason {
    match (enforces, reason) {
        (true, DecisionReason::WouldQuarantine) => DecisionReason::Quarantined,
        (true, DecisionReason::WouldDeny) => DecisionReason::Denied,
        (false, DecisionReason::Quarantined) => DecisionReason::WouldQuarantine,
        (false, DecisionReason::Denied) => DecisionReason::WouldDeny,
        (_, r) => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> AdmittedSchema {
        compile(
            &json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "mode": {"type": "string", "enum": ["read", "write"]}
                },
                "additionalProperties": false
            }),
            &ResourceLimits::initial(),
        )
        .unwrap()
    }

    #[test]
    fn validates_ok_and_hashes_stable() {
        let schema = sample();
        let a = schema.validate_value(&json!({"mode": "read", "path": "/tmp"}));
        let b = schema.validate_value(&json!({"path": "/tmp", "mode": "read"}));
        assert!(a.ok);
        assert_eq!(a.args_hash, b.args_hash);
    }

    #[test]
    fn rejects_bad_args_without_values() {
        let schema = sample();
        let r = schema.validate_value(&json!({"path": 1}));
        assert!(!r.ok);
        for e in &r.errors {
            assert!(!e.message.contains('1'));
            assert!(!e.display().contains("/tmp"));
        }
    }

    #[test]
    fn duplicate_keys_rejected() {
        let raw = br#"{"a":1,"a":2}"#;
        let err = parse_arguments(raw, &ResourceLimits::initial()).unwrap_err();
        assert!(matches!(err, SchemaError::DuplicateObjectKey(_)));
    }

    #[test]
    fn oversized_args_rejected_before_schema() {
        let limits = ResourceLimits {
            encoded_arguments_size: 16,
            ..ResourceLimits::initial()
        };
        let raw = br#"{"path":"this-is-longer-than-sixteen-bytes"}"#;
        assert!(matches!(
            parse_arguments(raw, &limits),
            Err(SchemaError::ArgumentsOversized)
        ));
    }

    #[test]
    fn combinators_work() {
        let schema = compile(
            &json!({
                "type": "object",
                "properties": {
                    "x": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "number"}
                        ]
                    }
                }
            }),
            &ResourceLimits::initial(),
        )
        .unwrap();
        assert!(schema.validate_value(&json!({"x": "a"})).ok);
        assert!(schema.validate_value(&json!({"x": 3})).ok);
        assert!(!schema.validate_value(&json!({"x": true})).ok);
    }

    #[test]
    fn apply_mode_preserves_decision_family() {
        assert_eq!(
            apply_mode(true, DecisionReason::WouldDeny),
            DecisionReason::Denied
        );
        assert_eq!(
            apply_mode(false, DecisionReason::Denied),
            DecisionReason::WouldDeny
        );
    }
}
