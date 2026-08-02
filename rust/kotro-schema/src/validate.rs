//! Compile admitted schemas and validate tool-call arguments.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    mpsc::{self, SyncSender, TrySendError},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use jsonschema::Draft;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::admit::admit_schema;
use crate::error::{DecisionReason, SanitizedError, SchemaError};
use crate::jcs;
use crate::limits::ResourceLimits;

const VALIDATION_WORKERS: usize = 4;
const VALIDATION_QUEUE_CAPACITY: usize = 16;
const DUPLICATE_KEY_MARKER: &str = "kotro_duplicate_object_key";

static SCHEMA_POOL: OnceLock<Result<SchemaPool, ()>> = OnceLock::new();

struct SchemaPool {
    sender: SyncSender<SchemaJob>,
}

enum SchemaJob {
    Compile(CompileJob),
    Validate(ValidationJob),
}

struct CompileJob {
    schema: Value,
    deadline: Instant,
    response: mpsc::Sender<CompileWorkerResult>,
}

struct ValidationJob {
    validator: Arc<jsonschema::Validator>,
    args: Value,
    max_errors: u32,
    deadline: Instant,
    response: mpsc::Sender<WorkerResult>,
}

enum WorkerResult {
    Complete(Vec<SanitizedError>),
    DeadlineExceeded,
    Unavailable,
}

enum CompileWorkerResult {
    Complete(Result<jsonschema::Validator, String>),
    DeadlineExceeded,
    Unavailable,
}

impl SchemaPool {
    fn new() -> Result<Self, ()> {
        let (sender, receiver) = mpsc::sync_channel::<SchemaJob>(VALIDATION_QUEUE_CAPACITY);
        let receiver = Arc::new(Mutex::new(receiver));

        for index in 0..VALIDATION_WORKERS {
            let receiver = Arc::clone(&receiver);
            std::thread::Builder::new()
                .name(format!("kotro-schema-{index}"))
                .spawn(move || schema_worker(receiver))
                .map_err(|_| ())?;
        }

        Ok(Self { sender })
    }

    fn submit(&self, job: SchemaJob) -> Result<(), ()> {
        self.sender.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) | TrySendError::Disconnected(_) => (),
        })
    }
}

fn schema_pool() -> Result<&'static SchemaPool, ()> {
    SCHEMA_POOL
        .get_or_init(SchemaPool::new)
        .as_ref()
        .map_err(|_| ())
}

fn schema_worker(receiver: Arc<Mutex<mpsc::Receiver<SchemaJob>>>) {
    loop {
        let job = {
            let Ok(receiver) = receiver.lock() else {
                return;
            };
            let Ok(job) = receiver.recv() else {
                return;
            };
            job
        };

        match job {
            SchemaJob::Compile(job) => compile_schema(job),
            SchemaJob::Validate(job) => validate_arguments(job),
        }
    }
}

fn compile_schema(job: CompileJob) {
    if Instant::now() >= job.deadline {
        let _ = job.response.send(CompileWorkerResult::DeadlineExceeded);
        return;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        jsonschema::options()
            .with_draft(Draft::Draft202012)
            .should_validate_formats(true)
            .should_ignore_unknown_formats(false)
            .build(&job.schema)
            .map_err(|error| sanitize_compile_error(&error.to_string()))
    }));
    let result = match result {
        Ok(_) if Instant::now() >= job.deadline => CompileWorkerResult::DeadlineExceeded,
        Ok(result) => CompileWorkerResult::Complete(result),
        Err(_) => CompileWorkerResult::Unavailable,
    };
    let _ = job.response.send(result);
}

fn validate_arguments(job: ValidationJob) {
    if Instant::now() >= job.deadline {
        let _ = job.response.send(WorkerResult::DeadlineExceeded);
        return;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut errors = Vec::new();
        let mut deadline_exceeded = false;
        // Retain at least one error so a caller-provided zero does not turn an
        // invalid instance into a successful validation.
        let max_errors = job.max_errors.max(1);
        for error in job.validator.iter_errors(&job.args) {
            if Instant::now() >= job.deadline {
                deadline_exceeded = true;
                break;
            }
            if errors.len() as u32 >= max_errors {
                break;
            }
            errors.push(sanitize_jsonschema_error(&error));
        }

        if deadline_exceeded || Instant::now() >= job.deadline {
            WorkerResult::DeadlineExceeded
        } else {
            WorkerResult::Complete(errors)
        }
    }));
    let result = match result {
        Ok(result) => result,
        Err(_) => WorkerResult::Unavailable,
    };
    let _ = job.response.send(result);
}

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

    let timeout = Duration::from_millis(limits.schema_compile_ms);
    let deadline = Instant::now() + timeout;
    let (response, result) = mpsc::channel();
    let job = SchemaJob::Compile(CompileJob {
        schema: schema.clone(),
        deadline,
        response,
    });
    schema_pool()
        .and_then(|pool| pool.submit(job))
        .map_err(|()| SchemaError::CompileTimeout)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let validator = match result.recv_timeout(remaining) {
        Ok(CompileWorkerResult::Complete(Ok(validator))) => validator,
        Ok(CompileWorkerResult::Complete(Err(error))) => {
            return Err(SchemaError::CompileFailed(error));
        }
        Ok(CompileWorkerResult::DeadlineExceeded | CompileWorkerResult::Unavailable) | Err(_) => {
            return Err(SchemaError::CompileTimeout);
        }
    };

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
        let encoded_len = serde_json::to_vec(args)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
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

        let timeout = Duration::from_millis(self.limits.validation_deadline_ms);
        let deadline = Instant::now() + timeout;
        let (response, result) = mpsc::channel();
        let job = SchemaJob::Validate(ValidationJob {
            validator: Arc::clone(&self.validator),
            args: args.clone(),
            max_errors: self.limits.validation_errors_retained,
            deadline,
            response,
        });
        let worker_result = schema_pool()
            .and_then(|pool| pool.submit(job))
            .and_then(|()| {
                let remaining = deadline.saturating_duration_since(Instant::now());
                result.recv_timeout(remaining).map_err(|_| ())
            });
        let errors = match worker_result {
            Ok(WorkerResult::Complete(errors)) => errors,
            Ok(WorkerResult::DeadlineExceeded | WorkerResult::Unavailable) | Err(()) => {
                return validation_unavailable();
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
    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = NoDuplicateValue
        .deserialize(&mut deserializer)
        .map_err(map_deserialize_error)?;
    deserializer.end().map_err(map_deserialize_error)?;
    Ok(value)
}

/// Parse JSON arguments, rejecting duplicate object keys and oversized payloads
/// before schema evaluation.
pub fn parse_arguments(raw: &[u8], limits: &ResourceLimits) -> Result<Value, SchemaError> {
    if raw.len() > limits.encoded_arguments_size {
        return Err(SchemaError::ArgumentsOversized);
    }
    let value = parse_json_rejecting_duplicates(raw)?;
    check_argument_structure(&value, limits)?;
    Ok(value)
}

fn validation_unavailable() -> ValidationResult {
    ValidationResult {
        ok: false,
        reason: DecisionReason::ValidationUnavailable,
        errors: vec![],
        args_hash: None,
        short_args_hash: None,
        detail: "validation_unavailable".into(),
    }
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

struct NoDuplicateValue;

impl<'de> DeserializeSeed<'de> for NoDuplicateValue {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(NoDuplicateValueVisitor)
    }
}

struct NoDuplicateValueVisitor;

impl<'de> Visitor<'de> for NoDuplicateValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        NoDuplicateValue.deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(NoDuplicateValue)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut seen = HashSet::new();
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(DUPLICATE_KEY_MARKER));
            }
            values.insert(key, object.next_value_seed(NoDuplicateValue)?);
        }
        Ok(Value::Object(values))
    }
}

fn map_deserialize_error(error: serde_json::Error) -> SchemaError {
    if error.to_string().contains(DUPLICATE_KEY_MARKER) {
        SchemaError::DuplicateObjectKey("redacted".into())
    } else {
        SchemaError::MalformedJson(error.to_string())
    }
}

fn sanitize_compile_error(msg: &str) -> String {
    // Drop any accidental instance snippets; keep keyword-ish text only.
    let trimmed = if msg.len() > 200 { &msg[..200] } else { msg };
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect()
}

fn sanitize_jsonschema_error(err: &jsonschema::ValidationError<'_>) -> SanitizedError {
    let path = err.instance_path().to_string();
    let path = if path.is_empty() {
        "$".into()
    } else {
        format!("${path}")
    };
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
    fn zero_error_retention_cannot_make_invalid_arguments_valid() {
        let limits = ResourceLimits {
            validation_errors_retained: 0,
            ..ResourceLimits::initial()
        };
        let schema = compile(
            &json!({
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}}
            }),
            &limits,
        )
        .unwrap();
        let result = schema.validate_value(&json!({"path": 1}));
        assert!(!result.ok);
        assert_eq!(result.reason, DecisionReason::WouldDeny);
    }

    #[test]
    fn duplicate_keys_rejected() {
        let raw = br#"{"a":1,"a":2}"#;
        let err = parse_arguments(raw, &ResourceLimits::initial()).unwrap_err();
        assert!(matches!(err, SchemaError::DuplicateObjectKey(_)));
    }

    #[test]
    fn escaped_duplicate_keys_rejected_after_decoding() {
        let raw = br#"{"outer":{"a":1,"\u0061":2}}"#;
        let err = parse_arguments(raw, &ResourceLimits::initial()).unwrap_err();
        assert!(matches!(err, SchemaError::DuplicateObjectKey(_)));
    }

    #[test]
    fn distinct_escaped_keys_are_preserved() {
        let raw = br#"{"a":1,"\u0062":2}"#;
        let parsed = parse_arguments(raw, &ResourceLimits::initial()).unwrap();
        assert_eq!(parsed, json!({"a": 1, "b": 2}));
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
