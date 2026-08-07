//! TaskEnvelope verification, non-expansion, and authority intersection.

use std::collections::HashSet;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::envelope::{
    envelope_digest, key_id_for_public_key, signing_input, ArgumentConstraint, Capabilities,
    DestinationCapability, TaskEnvelope, API_VERSION_V1ALPHA1, API_VERSION_V1ALPHA2,
    HARD_MAX_DEPTH, KIND, MAX_ENVELOPE_BYTES,
};
use crate::reason::TaskReason;
use crate::trust::{ParentStore, TrustStore};

#[derive(Clone)]
pub struct VerificationContext<'a> {
    pub trust: &'a TrustStore,
    pub parents: &'a dyn ParentStore,
    pub now_rfc3339: &'a str,
    /// Runtime deployment audience that must exactly match the envelope.
    pub expected_audience: Option<&'a str>,
    pub kill_engaged: bool,
}

#[derive(Debug, Clone)]
pub struct VerifiedAuthority {
    pub envelope: TaskEnvelope,
    pub digest: String,
    pub effective: Capabilities,
}

impl VerifiedAuthority {
    /// Whether `server`/`tool` (with optional schema digest and args hash) is
    /// within the effective capability set. Fail-closed: an empty tools list
    /// denies every tool call.
    pub fn allows_tool(
        &self,
        server: &str,
        tool: &str,
        args_hash: Option<&str>,
        schema_digest: Option<&str>,
    ) -> Result<(), TaskReason> {
        let caps: Vec<_> = self
            .effective
            .tools
            .iter()
            .filter(|t| t.server == server && t.name == tool)
            .collect();
        if caps.is_empty() {
            return Err(TaskReason::TaskActionOutOfScope);
        }
        // Any matching capability that accepts this call shape is enough.
        let mut last = TaskReason::TaskActionOutOfScope;
        for cap in caps {
            if let Some(want) = cap.tool_schema_sha256.as_deref() {
                match schema_digest {
                    Some(got) if digests_equal(want, got) => {}
                    _ => {
                        last = TaskReason::TaskActionOutOfScope;
                        continue;
                    }
                }
            }
            match &cap.arguments {
                None | Some(crate::envelope::ArgumentConstraint::Any) => return Ok(()),
                Some(crate::envelope::ArgumentConstraint::Exact { hashes }) => {
                    let Some(hash) = args_hash else {
                        last = TaskReason::TaskActionOutOfScope;
                        continue;
                    };
                    if hashes.iter().any(|h| digests_equal(h, hash)) {
                        return Ok(());
                    }
                    last = TaskReason::TaskActionOutOfScope;
                }
            }
        }
        Err(last)
    }

    /// Remaining tool-call budget when budgets are declared.
    pub fn max_tool_calls(&self) -> Option<u64> {
        self.effective.budgets.as_ref().map(|b| b.max_tool_calls)
    }
}

pub fn parse_envelope_bytes(raw: &[u8]) -> Result<TaskEnvelope, TaskReason> {
    if raw.len() > MAX_ENVELOPE_BYTES {
        return Err(TaskReason::TaskMalformed);
    }
    let text = std::str::from_utf8(raw).map_err(|_| TaskReason::TaskMalformed)?;
    let value = if looks_like_yaml(text) {
        parse_restricted_yaml(text)?
    } else {
        parse_json_rejecting_duplicates(raw)?
    };
    // `deny_unknown_fields` on TaskEnvelope rejects undeclared properties.
    serde_json::from_value(value).map_err(|_| TaskReason::TaskSchemaInvalid)
}

fn looks_like_yaml(text: &str) -> bool {
    let trimmed = text.trim_start();
    !trimmed.starts_with('{') && !trimmed.starts_with('[')
}

fn parse_restricted_yaml(text: &str) -> Result<Value, TaskReason> {
    // YAML 1.2 JSON-compatible subset: no anchors, aliases, merge keys, custom tags.
    if text.contains("<<:")
        || text.contains("!!")
        || text.contains("\n&")
        || text.contains(" *")
        || text.contains("&")
        || text.contains("*")
    {
        return Err(TaskReason::TaskMalformed);
    }
    let mut documents = serde_yaml::Deserializer::from_str(text);
    let document = documents.next().ok_or(TaskReason::TaskMalformed)?;
    let v = NoDuplicateValue
        .deserialize(document)
        .map_err(|_| TaskReason::TaskMalformed)?;
    if documents.next().is_some() {
        return Err(TaskReason::TaskMalformed);
    }
    reject_non_json_yaml(&v)?;
    Ok(v)
}

fn reject_non_json_yaml(v: &Value) -> Result<(), TaskReason> {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k.is_empty() {
                    return Err(TaskReason::TaskMalformed);
                }
                reject_non_json_yaml(child)?;
            }
            Ok(())
        }
        Value::Array(arr) => {
            for child in arr {
                reject_non_json_yaml(child)?;
            }
            Ok(())
        }
        Value::Number(n) => {
            if n.as_i64().is_none() && n.as_u64().is_none() {
                return Err(TaskReason::TaskMalformed);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn parse_json_rejecting_duplicates(raw: &[u8]) -> Result<Value, TaskReason> {
    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = NoDuplicateValue
        .deserialize(&mut deserializer)
        .map_err(|_| TaskReason::TaskMalformed)?;
    deserializer.end().map_err(|_| TaskReason::TaskMalformed)?;
    Ok(value)
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
        formatter.write_str("a JSON-compatible value without duplicate object keys")
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
                return Err(de::Error::custom("duplicate object key"));
            }
            values.insert(key, object.next_value_seed(NoDuplicateValue)?);
        }
        Ok(Value::Object(values))
    }
}

pub fn verify(
    envelope: &TaskEnvelope,
    ctx: &VerificationContext<'_>,
) -> Result<VerifiedAuthority, TaskReason> {
    if ctx.kill_engaged {
        return Err(TaskReason::TaskActionOutOfScope);
    }
    if let Some(aud) = ctx.expected_audience {
        if envelope.audience != aud {
            return Err(TaskReason::TaskActionOutOfScope);
        }
    }
    validate_shape(envelope)?;
    let digest = envelope_digest(envelope).map_err(|_| TaskReason::TaskMalformed)?;
    if ctx.trust.is_revoked_digest(&digest) || ctx.trust.is_revoked_task(&envelope.task_id) {
        return Err(TaskReason::TaskKeyRevoked);
    }

    verify_signature(envelope, ctx)?;
    check_time(envelope, ctx.now_rfc3339)?;

    let effective = if let Some(parent_ref) = &envelope.parent {
        let parent = ctx
            .parents
            .get(&parent_ref.digest)
            .ok_or(TaskReason::TaskParentMissing)?;
        let parent_digest = envelope_digest(&parent).map_err(|_| TaskReason::TaskMalformed)?;
        if parent_digest != parent_ref.digest {
            return Err(TaskReason::TaskParentDigestMismatch);
        }
        if parent.task_id != parent_ref.task_id {
            return Err(TaskReason::TaskParentDigestMismatch);
        }
        // Recurse; cycle detection via digest chain set.
        let mut seen = std::collections::HashSet::new();
        seen.insert(digest.clone());
        verify_chain(&parent, ctx, &mut seen)?;
        check_non_expansion(&parent, envelope)?;
        // Child signer must appear in parent.delegation.signers.
        if !parent
            .delegation
            .signers
            .iter()
            .any(|s| s.key_id == envelope.signature.key_id)
        {
            return Err(TaskReason::TaskKeyUntrusted);
        }
        intersect_capabilities(&parent.capabilities, &envelope.capabilities)?
    } else {
        // Root: resolve key from operator trust store.
        let _key = ctx.trust.find_active(
            &envelope.signature.key_id,
            &envelope.issuer,
            ctx.now_rfc3339,
        )?;
        envelope.capabilities.clone()
    };

    Ok(VerifiedAuthority {
        envelope: envelope.clone(),
        digest,
        effective,
    })
}

fn digests_equal(a: &str, b: &str) -> bool {
    let na = a.strip_prefix("sha256:").unwrap_or(a);
    let nb = b.strip_prefix("sha256:").unwrap_or(b);
    na == nb
}

fn verify_chain(
    envelope: &TaskEnvelope,
    ctx: &VerificationContext<'_>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<(), TaskReason> {
    let digest = envelope_digest(envelope).map_err(|_| TaskReason::TaskMalformed)?;
    if !seen.insert(digest.clone()) {
        return Err(TaskReason::TaskCycle);
    }
    validate_shape(envelope)?;
    verify_signature(envelope, ctx)?;
    check_time(envelope, ctx.now_rfc3339)?;
    if let Some(parent_ref) = &envelope.parent {
        let parent = ctx
            .parents
            .get(&parent_ref.digest)
            .ok_or(TaskReason::TaskParentMissing)?;
        verify_chain(&parent, ctx, seen)?;
        check_non_expansion(&parent, envelope)?;
    } else {
        let _ = ctx.trust.find_active(
            &envelope.signature.key_id,
            &envelope.issuer,
            ctx.now_rfc3339,
        )?;
    }
    Ok(())
}

fn validate_shape(envelope: &TaskEnvelope) -> Result<(), TaskReason> {
    if envelope.kind != KIND {
        return Err(TaskReason::TaskSchemaInvalid);
    }
    match envelope.api_version.as_str() {
        API_VERSION_V1ALPHA1 => {
            if envelope.repository.is_some() || envelope.land.is_some() {
                // v1alpha1 must not carry Permit fields (fail closed / no silent upgrade).
                return Err(TaskReason::TaskSchemaInvalid);
            }
        }
        API_VERSION_V1ALPHA2 => {
            let repo = envelope
                .repository
                .as_ref()
                .ok_or(TaskReason::TaskSchemaInvalid)?;
            let land = envelope.land.as_ref().ok_or(TaskReason::TaskSchemaInvalid)?;
            validate_repository(repo)?;
            let _ = land.mode;
        }
        _ => return Err(TaskReason::TaskSchemaInvalid),
    }
    if envelope.depth > HARD_MAX_DEPTH {
        return Err(TaskReason::TaskDelegationDepth);
    }
    if envelope.signature.algorithm != "Ed25519" {
        return Err(TaskReason::TaskSignatureInvalid);
    }
    if envelope.agent_scope.names.is_empty() {
        return Err(TaskReason::TaskSchemaInvalid);
    }
    // Deny wildcards.
    for n in &envelope.agent_scope.names {
        if n.contains('*') {
            return Err(TaskReason::TaskSchemaInvalid);
        }
    }
    for t in &envelope.capabilities.tools {
        if t.server.contains('*') || t.name.contains('*') {
            return Err(TaskReason::TaskSchemaInvalid);
        }
    }
    let budgets = envelope
        .capabilities
        .budgets
        .as_ref()
        .ok_or(TaskReason::TaskSchemaInvalid)?;
    let _ = budgets;
    Ok(())
}

fn validate_repository(repo: &crate::envelope::RepositoryAuthority) -> Result<(), TaskReason> {
    if repo.identity.trim().is_empty() || repo.base_ref.trim().is_empty() {
        return Err(TaskReason::TaskSchemaInvalid);
    }
    if !is_full_git_sha(&repo.source_pin) || !is_full_git_sha(&repo.base_sha) {
        return Err(TaskReason::TaskSchemaInvalid);
    }
    Ok(())
}

fn is_full_git_sha(s: &str) -> bool {
    let n = s.len();
    (n == 40 || n == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn verify_signature(
    envelope: &TaskEnvelope,
    ctx: &VerificationContext<'_>,
) -> Result<(), TaskReason> {
    let key_b64 = if envelope.parent.is_none() {
        ctx.trust
            .find_active(
                &envelope.signature.key_id,
                &envelope.issuer,
                ctx.now_rfc3339,
            )?
            .public_key
            .clone()
    } else {
        // Look up public key from parent delegation signers via parent store chain tip.
        // For the leaf, parent was already loaded by caller; here we search trust + any
        // parent delegation material available through trust store only for roots.
        // Child keys are carried in parent.delegation.signers — recovered below.
        resolve_child_public_key(envelope, ctx)?
    };

    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(key_b64.as_bytes())
        .map_err(|_| TaskReason::TaskKeyUntrusted)?;
    if raw.len() != 32 {
        return Err(TaskReason::TaskKeyUntrusted);
    }
    let expected_id = key_id_for_public_key(&raw);
    if expected_id != envelope.signature.key_id {
        return Err(TaskReason::TaskKeyUntrusted);
    }
    let vk = VerifyingKey::from_bytes(raw.as_slice().try_into().unwrap())
        .map_err(|_| TaskReason::TaskKeyUntrusted)?;
    let sig_raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(envelope.signature.value.as_bytes())
        .map_err(|_| TaskReason::TaskSignatureInvalid)?;
    let sig = Signature::from_slice(&sig_raw).map_err(|_| TaskReason::TaskSignatureInvalid)?;
    let msg = signing_input(envelope).map_err(|_| TaskReason::TaskMalformed)?;
    vk.verify(&msg, &sig)
        .map_err(|_| TaskReason::TaskSignatureInvalid)?;
    Ok(())
}

fn resolve_child_public_key(
    envelope: &TaskEnvelope,
    ctx: &VerificationContext<'_>,
) -> Result<String, TaskReason> {
    let parent_ref = envelope
        .parent
        .as_ref()
        .ok_or(TaskReason::TaskParentMissing)?;
    let parent = ctx
        .parents
        .get(&parent_ref.digest)
        .ok_or(TaskReason::TaskParentMissing)?;
    parent
        .delegation
        .signers
        .iter()
        .find(|s| s.key_id == envelope.signature.key_id)
        .map(|s| s.public_key.clone())
        .ok_or(TaskReason::TaskKeyUntrusted)
}

fn check_time(envelope: &TaskEnvelope, now: &str) -> Result<(), TaskReason> {
    check_envelope_time_window(
        &envelope.issued_at,
        &envelope.not_before,
        &envelope.expires_at,
        now,
    )
}

/// Validate envelope time fields.
///
/// Contract (Permit / Sol P1.4):
/// - Parse all timestamps as RFC3339 instants (offset-safe).
/// - Require `issued_at <= not_before < expires_at`.
/// - Validity is half-open `[not_before, expires_at)` — `now == expires_at` is expired.
pub fn check_envelope_time_window(
    issued_at: &str,
    not_before: &str,
    expires_at: &str,
    now: &str,
) -> Result<(), TaskReason> {
    let issued = parse_rfc3339(issued_at)?;
    let nbf = parse_rfc3339(not_before)?;
    let exp = parse_rfc3339(expires_at)?;
    let now_t = parse_rfc3339(now)?;
    if issued > nbf || nbf >= exp {
        return Err(TaskReason::TaskMalformed);
    }
    if now_t < nbf {
        return Err(TaskReason::TaskNotYetValid);
    }
    if now_t >= exp {
        return Err(TaskReason::TaskExpired);
    }
    Ok(())
}

pub fn parse_rfc3339(s: &str) -> Result<time::OffsetDateTime, TaskReason> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|_| TaskReason::TaskMalformed)
}

pub fn check_non_expansion(parent: &TaskEnvelope, child: &TaskEnvelope) -> Result<(), TaskReason> {
    if child.audience != parent.audience
        || child.issuer != parent.issuer
        || child.principal != parent.principal
    {
        return Err(TaskReason::TaskPrincipalMismatch);
    }
    if child.depth != parent.depth + 1 {
        return Err(TaskReason::TaskDelegationDepth);
    }
    if child.depth > HARD_MAX_DEPTH || child.depth > parent.delegation.max_depth {
        return Err(TaskReason::TaskDelegationDepth);
    }
    let child_nbf = parse_rfc3339(&child.not_before)?;
    let parent_nbf = parse_rfc3339(&parent.not_before)?;
    let child_exp = parse_rfc3339(&child.expires_at)?;
    let parent_exp = parse_rfc3339(&parent.expires_at)?;
    if child_nbf < parent_nbf || child_exp > parent_exp {
        return Err(TaskReason::TaskCapabilityExpansion);
    }
    // Agent scope must narrow.
    if !is_subset(&child.agent_scope.names, &parent.agent_scope.names) {
        return Err(TaskReason::TaskAgentMismatch);
    }
    if !is_subset(
        &child.agent_scope.workload_identities,
        &parent.agent_scope.workload_identities,
    ) && !parent.agent_scope.workload_identities.is_empty()
    {
        return Err(TaskReason::TaskAgentMismatch);
    }
    if child.delegation.max_depth > parent.delegation.max_depth {
        return Err(TaskReason::TaskCapabilityExpansion);
    }
    if !child.delegation.signers.iter().all(|s| {
        parent
            .delegation
            .signers
            .iter()
            .any(|p| p.key_id == s.key_id)
    }) {
        return Err(TaskReason::TaskCapabilityExpansion);
    }
    check_repository_land_narrow(parent, child)?;
    check_caps_narrow(&parent.capabilities, &child.capabilities)?;
    Ok(())
}

fn check_repository_land_narrow(
    parent: &TaskEnvelope,
    child: &TaskEnvelope,
) -> Result<(), TaskReason> {
    match (&parent.repository, &child.repository) {
        (None, None) => {}
        (Some(p), Some(c)) => {
            if c.identity != p.identity
                || c.source_pin != p.source_pin
                || c.base_ref != p.base_ref
                || c.base_sha != p.base_sha
            {
                return Err(TaskReason::TaskCapabilityExpansion);
            }
        }
        // Child cannot introduce or drop Permit repository authority relative to parent.
        _ => return Err(TaskReason::TaskCapabilityExpansion),
    }
    match (&parent.land, &child.land) {
        (None, None) => {}
        (Some(p), Some(c)) => {
            if !c.mode.is_narrower_or_equal(p.mode) {
                return Err(TaskReason::TaskCapabilityExpansion);
            }
        }
        _ => return Err(TaskReason::TaskCapabilityExpansion),
    }
    Ok(())
}

fn check_caps_narrow(parent: &Capabilities, child: &Capabilities) -> Result<(), TaskReason> {
    for ct in &child.tools {
        let Some(pt) = parent
            .tools
            .iter()
            .find(|p| p.server == ct.server && p.name == ct.name)
        else {
            return Err(TaskReason::TaskCapabilityExpansion);
        };
        match (&pt.tool_schema_sha256, &ct.tool_schema_sha256) {
            (Some(p), Some(c)) if p != c => return Err(TaskReason::TaskCapabilityExpansion),
            (Some(_), None) => return Err(TaskReason::TaskCapabilityExpansion),
            _ => {}
        }
        match (&pt.arguments, &ct.arguments) {
            (
                Some(ArgumentConstraint::Exact { hashes: ph }),
                Some(ArgumentConstraint::Exact { hashes: ch }),
            ) => {
                if !ch.iter().all(|h| ph.contains(h)) {
                    return Err(TaskReason::TaskCapabilityExpansion);
                }
            }
            (Some(ArgumentConstraint::Exact { .. }), Some(ArgumentConstraint::Any)) => {
                return Err(TaskReason::TaskCapabilityExpansion);
            }
            (Some(ArgumentConstraint::Exact { .. }), None) => {
                return Err(TaskReason::TaskCapabilityExpansion);
            }
            _ => {}
        }
    }
    for cm in &child.models {
        let Some(pm) = parent
            .models
            .iter()
            .find(|p| p.provider == cm.provider && p.name == cm.name)
        else {
            return Err(TaskReason::TaskCapabilityExpansion);
        };
        match (&pm.version, &cm.version) {
            (Some(p), Some(c)) if p != c => return Err(TaskReason::TaskCapabilityExpansion),
            (Some(_), None) => return Err(TaskReason::TaskCapabilityExpansion),
            _ => {}
        }
    }
    for cd in &child.destinations {
        if !parent
            .destinations
            .iter()
            .any(|pd| destination_narrows(pd, cd))
        {
            return Err(TaskReason::TaskCapabilityExpansion);
        }
    }
    for cc in &child.credentials {
        let Some(pc) = parent.credentials.iter().find(|p| p.id == cc.id) else {
            return Err(TaskReason::TaskCapabilityExpansion);
        };
        if !is_subset(&cc.scopes, &pc.scopes) {
            return Err(TaskReason::TaskCapabilityExpansion);
        }
    }
    for cf in &child.filesystem {
        let Some(pf) = parent
            .filesystem
            .iter()
            .find(|p| path_is_descendant(&p.root, &cf.root))
        else {
            return Err(TaskReason::TaskCapabilityExpansion);
        };
        if !is_subset(&cf.operations, &pf.operations) {
            return Err(TaskReason::TaskCapabilityExpansion);
        }
    }
    match (&parent.budgets, &child.budgets) {
        (Some(pb), Some(cb)) => {
            if cb.max_tool_calls > pb.max_tool_calls
                || cb.max_model_calls > pb.max_model_calls
                || cb.max_input_tokens > pb.max_input_tokens
                || cb.max_output_tokens > pb.max_output_tokens
                || cb.max_cost_microusd > pb.max_cost_microusd
                || cb.max_duration_seconds > pb.max_duration_seconds
            {
                return Err(TaskReason::TaskCapabilityExpansion);
            }
        }
        (None, Some(_)) => return Err(TaskReason::TaskCapabilityExpansion),
        _ => {}
    }
    Ok(())
}

fn destination_narrows(parent: &DestinationCapability, child: &DestinationCapability) -> bool {
    if parent.scheme != child.scheme
        || !parent.host.eq_ignore_ascii_case(&child.host)
        || parent.port != child.port
    {
        return false;
    }
    match (&parent.path_prefix, &child.path_prefix) {
        (None, _) => true,
        (Some(p), Some(c)) => match (normalize_url_path(p), normalize_url_path(c)) {
            (Some(p), Some(c)) => path_prefix_descendant(&p, &c),
            _ => false,
        },
        (Some(_), None) => false,
    }
}

fn path_is_descendant(parent_root: &str, child_root: &str) -> bool {
    match (
        normalize_fs_path(parent_root),
        normalize_fs_path(child_root),
    ) {
        (Some(p), Some(c)) => path_prefix_descendant(&p, &c),
        _ => false,
    }
}

/// Exact match or child is under parent with a path-segment boundary
/// (`/repos/foo` does not authorize `/repos/foobar`).
fn path_prefix_descendant(parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }
    let p = parent.trim_end_matches('/');
    child.starts_with(p) && child[p.len()..].starts_with('/')
}

fn normalize_url_path(path: &str) -> Option<String> {
    if !path.starts_with('/') {
        return None;
    }
    let mut stack: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if stack.is_empty() {
                    return None;
                }
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    Some(format!("/{}", stack.join("/")))
}

fn normalize_fs_path(path: &str) -> Option<String> {
    // Absolute paths only; reject null bytes and empty.
    if path.is_empty() || !path.starts_with('/') || path.contains('\0') {
        return None;
    }
    normalize_url_path(path)
}

fn is_subset(child: &[String], parent: &[String]) -> bool {
    child.iter().all(|c| parent.iter().any(|p| p == c))
}

pub fn intersect_capabilities(
    parent: &Capabilities,
    child: &Capabilities,
) -> Result<Capabilities, TaskReason> {
    check_caps_narrow(parent, child)?;
    // Defense in depth: effective authority is the child (already proven ⊆ parent).
    Ok(child.clone())
}

/// Sign an envelope in-place (test / authority helper). Does not load private keys
/// from the trust store — caller supplies the signing key.
pub fn sign_envelope(
    envelope: &mut TaskEnvelope,
    signing_key: &ed25519_dalek::SigningKey,
) -> Result<(), TaskReason> {
    use ed25519_dalek::Signer;
    let vk = signing_key.verifying_key();
    let key_id = key_id_for_public_key(vk.as_bytes());
    envelope.signature.key_id = key_id;
    envelope.signature.algorithm = "Ed25519".into();
    let msg = signing_input(envelope).map_err(|_| TaskReason::TaskMalformed)?;
    let sig = signing_key.sign(&msg);
    envelope.signature.value =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes());
    Ok(())
}

pub fn public_key_b64(vk: &VerifyingKey) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(vk.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::*;
    use crate::trust::{MemoryParentStore, TrustKey, TrustStore};
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn budgets() -> Budgets {
        Budgets {
            max_tool_calls: 100,
            max_model_calls: 40,
            max_input_tokens: 500_000,
            max_output_tokens: 100_000,
            max_cost_microusd: 5_000_000,
            max_duration_seconds: 3600,
        }
    }

    fn sample_envelope(sk: &SigningKey) -> TaskEnvelope {
        let vk = sk.verifying_key();
        let mut env = TaskEnvelope {
            api_version: API_VERSION.into(),
            kind: KIND.into(),
            task_id: "task-root".into(),
            audience: "kotro://deployment/acme".into(),
            issuer: "kotro://authority/acme".into(),
            principal: EnvelopePrincipal {
                subject: "user@example.com".into(),
                issuer: "https://identity.example.com".into(),
            },
            agent_scope: AgentScope {
                names: vec!["codex".into()],
                workload_identities: vec!["spiffe://example.com/agent/codex".into()],
            },
            issued_at: "2026-08-01T18:00:00Z".into(),
            not_before: "2026-08-01T18:00:00Z".into(),
            expires_at: "2026-08-01T19:00:00Z".into(),
            nonce: "AAAAAAAAAAAAAAAAAAAAAA".into(),
            depth: 0,
            parent: None,
            repository: None,
            land: None,
            capabilities: Capabilities {
                tools: vec![ToolCapability {
                    server: "github".into(),
                    name: "create_issue".into(),
                    tool_schema_sha256: None,
                    arguments: Some(ArgumentConstraint::Any),
                }],
                models: vec![],
                destinations: vec![DestinationCapability {
                    scheme: "https".into(),
                    host: "api.github.com".into(),
                    port: 443,
                    path_prefix: Some("/repos/kotro-labs/".into()),
                }],
                credentials: vec![],
                filesystem: vec![],
                budgets: Some(budgets()),
            },
            delegation: Delegation {
                max_depth: 2,
                signers: vec![DelegationSigner {
                    key_id: key_id_for_public_key(vk.as_bytes()),
                    public_key: public_key_b64(&vk),
                }],
            },
            signature: EnvelopeSignature {
                algorithm: "Ed25519".into(),
                key_id: String::new(),
                value: String::new(),
            },
        };
        sign_envelope(&mut env, sk).unwrap();
        env
    }

    fn trust_for(sk: &SigningKey) -> TrustStore {
        let vk = sk.verifying_key();
        TrustStore {
            keys: vec![TrustKey {
                key_id: key_id_for_public_key(vk.as_bytes()),
                algorithm: "Ed25519".into(),
                public_key: public_key_b64(&vk),
                issuers: vec!["kotro://authority/acme".into()],
                status: "active".into(),
                not_before: "2026-01-01T00:00:00Z".into(),
                not_after: "2027-01-01T00:00:00Z".into(),
            }],
            revoked_key_ids: vec![],
            revoked_task_ids: vec![],
            revoked_envelope_digests: vec![],
        }
    }

    #[test]
    fn valid_root_verifies() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_envelope(&sk);
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert!(verify(&env, &ctx).is_ok());
    }

    #[test]
    fn key_reorder_same_signing_input() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_envelope(&sk);
        let a = signing_input(&env).unwrap();
        let v = serde_json::to_value(&env).unwrap();
        // Round-trip through value/struct should keep canonical signing input stable.
        let env2: TaskEnvelope = serde_json::from_value(v).unwrap();
        let b = signing_input(&env2).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn tamper_breaks_signature() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_envelope(&sk);
        env.task_id = "task-tampered".into();
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert_eq!(
            verify(&env, &ctx).unwrap_err(),
            TaskReason::TaskSignatureInvalid
        );
    }

    #[test]
    fn expired_fails() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_envelope(&sk);
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T20:00:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert_eq!(verify(&env, &ctx).unwrap_err(), TaskReason::TaskExpired);
    }

    #[test]
    fn child_cannot_expand_budget() {
        let sk = SigningKey::generate(&mut OsRng);
        let parent = sample_envelope(&sk);
        let mut child = parent.clone();
        child.task_id = "task-child".into();
        child.depth = 1;
        child.parent = Some(ParentRef {
            task_id: parent.task_id.clone(),
            digest: envelope_digest(&parent).unwrap(),
        });
        child.capabilities.budgets.as_mut().unwrap().max_tool_calls = 10_000;
        assert_eq!(
            check_non_expansion(&parent, &child).unwrap_err(),
            TaskReason::TaskCapabilityExpansion
        );
    }

    #[test]
    fn child_can_narrow_destination_prefix() {
        let sk = SigningKey::generate(&mut OsRng);
        let parent = sample_envelope(&sk);
        let mut child = parent.clone();
        child.depth = 1;
        child.capabilities.destinations[0].path_prefix =
            Some("/repos/kotro-labs/kotro-proxy-engine/".into());
        assert!(check_non_expansion(&parent, &child).is_ok());
    }

    #[test]
    fn rejects_unknown_fields() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_envelope(&sk);
        let mut v = serde_json::to_value(&env).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("extra".into(), serde_json::json!(true));
        let raw = serde_json::to_vec(&v).unwrap();
        assert_eq!(
            parse_envelope_bytes(&raw).unwrap_err(),
            TaskReason::TaskSchemaInvalid
        );
    }

    #[test]
    fn rejects_duplicate_json_keys() {
        let raw = br#"{"api_version":"kotro.dev/v1alpha1","api_version":"x"}"#;
        assert_eq!(
            parse_envelope_bytes(raw).unwrap_err(),
            TaskReason::TaskMalformed
        );
    }

    #[test]
    fn rejects_escaped_duplicate_json_keys() {
        let raw = br#"{"api_version":"kotro.dev/v1alpha1","api_\u0076ersion":"x"}"#;
        assert_eq!(
            parse_envelope_bytes(raw).unwrap_err(),
            TaskReason::TaskMalformed
        );
    }

    #[test]
    fn rejects_duplicate_yaml_keys() {
        let raw = b"api_version: kotro.dev/v1alpha1\napi_version: x\n";
        assert_eq!(
            parse_envelope_bytes(raw).unwrap_err(),
            TaskReason::TaskMalformed
        );
    }

    #[test]
    fn offset_timestamp_expiry_is_correct() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_envelope(&sk);
        // expires at 19:00Z == 12:00-07:00
        env.expires_at = "2026-08-01T12:00:00-07:00".into();
        sign_envelope(&mut env, &sk).unwrap();
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert!(verify(&env, &ctx).is_ok());
        let ctx_expired = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T19:00:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert_eq!(
            verify(&env, &ctx_expired).unwrap_err(),
            TaskReason::TaskExpired
        );
    }

    #[test]
    fn audience_binding_required() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_envelope(&sk);
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/other"),
            kill_engaged: false,
        };
        assert_eq!(
            verify(&env, &ctx).unwrap_err(),
            TaskReason::TaskActionOutOfScope
        );
    }

    #[test]
    fn path_traversal_and_prefix_bypass_rejected() {
        assert!(!path_is_descendant("/safe", "/safe/../etc"));
        assert!(!path_is_descendant("/repos/foo", "/repos/foobar"));
        assert!(path_is_descendant("/repos/foo", "/repos/foo/bar"));
        let parent = DestinationCapability {
            scheme: "https".into(),
            host: "api.github.com".into(),
            port: 443,
            path_prefix: Some("/repos/foo".into()),
        };
        let evil = DestinationCapability {
            scheme: "https".into(),
            host: "api.github.com".into(),
            port: 443,
            path_prefix: Some("/repos/foobar".into()),
        };
        assert!(!destination_narrows(&parent, &evil));
    }

    #[test]
    fn json_and_yaml_same_signing_input() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_envelope(&sk);
        let json = serde_json::to_vec(&env).unwrap();
        let parsed_json = parse_envelope_bytes(&json).unwrap();
        let yaml = serde_yaml::to_string(&env).unwrap();
        let parsed_yaml = parse_envelope_bytes(yaml.as_bytes()).unwrap();
        assert_eq!(
            signing_input(&parsed_json).unwrap(),
            signing_input(&parsed_yaml).unwrap()
        );
    }

    #[test]
    fn allows_tool_respects_exact_args_and_schema() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_envelope(&sk);
        env.capabilities.tools = vec![ToolCapability {
            server: "files".into(),
            name: "read_file".into(),
            tool_schema_sha256: Some("sha256:schema1".into()),
            arguments: Some(ArgumentConstraint::Exact {
                hashes: vec!["sha256:args1".into()],
            }),
        }];
        sign_envelope(&mut env, &sk).unwrap();
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        let auth = verify(&env, &ctx).unwrap();
        assert!(auth
            .allows_tool(
                "files",
                "read_file",
                Some("sha256:args1"),
                Some("sha256:schema1")
            )
            .is_ok());
        assert_eq!(
            auth.allows_tool(
                "files",
                "read_file",
                Some("sha256:other"),
                Some("sha256:schema1")
            )
            .unwrap_err(),
            TaskReason::TaskActionOutOfScope
        );
        assert_eq!(
            auth.allows_tool(
                "files",
                "write_file",
                Some("sha256:args1"),
                Some("sha256:schema1")
            )
            .unwrap_err(),
            TaskReason::TaskActionOutOfScope
        );
    }

    #[test]
    fn time_window_exact_expiry_is_expired() {
        assert_eq!(
            check_envelope_time_window(
                "2026-08-01T18:00:00Z",
                "2026-08-01T18:00:00Z",
                "2026-08-01T19:00:00Z",
                "2026-08-01T19:00:00Z",
            )
            .unwrap_err(),
            TaskReason::TaskExpired
        );
    }

    #[test]
    fn time_window_offset_equivalent_now_ok() {
        // Same instant as 18:30Z expressed with +00:00 offset.
        assert!(check_envelope_time_window(
            "2026-08-01T18:00:00Z",
            "2026-08-01T18:00:00Z",
            "2026-08-01T19:00:00Z",
            "2026-08-01T18:30:00+00:00",
        )
        .is_ok());
    }

    #[test]
    fn time_window_rejects_malformed_ordering() {
        assert_eq!(
            check_envelope_time_window(
                "2026-08-01T18:30:00Z",
                "2026-08-01T18:00:00Z", // issued_at > not_before
                "2026-08-01T19:00:00Z",
                "2026-08-01T18:30:00Z",
            )
            .unwrap_err(),
            TaskReason::TaskMalformed
        );
        assert_eq!(
            check_envelope_time_window(
                "2026-08-01T18:00:00Z",
                "2026-08-01T19:00:00Z",
                "2026-08-01T19:00:00Z", // not_before >= expires_at
                "2026-08-01T19:00:00Z",
            )
            .unwrap_err(),
            TaskReason::TaskMalformed
        );
    }

    #[test]
    fn trust_key_window_parses_offsets_not_lexically() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let key_id = key_id_for_public_key(vk.as_bytes());
        // Lexically "Z" > "+00:00" in ASCII, but instants are equal — must parse.
        let trust = TrustStore {
            keys: vec![TrustKey {
                key_id: key_id.clone(),
                algorithm: "Ed25519".into(),
                public_key: public_key_b64(&vk),
                issuers: vec!["kotro://authority/acme".into()],
                status: "active".into(),
                not_before: "2026-08-01T00:00:00+00:00".into(),
                not_after: "2026-12-31T23:59:59Z".into(),
            }],
            revoked_key_ids: vec![],
            revoked_task_ids: vec![],
            revoked_envelope_digests: vec![],
        };
        assert!(trust
            .find_active(&key_id, "kotro://authority/acme", "2026-08-01T18:30:00Z")
            .is_ok());
    }

    fn sha40(nibble: char) -> String {
        std::iter::repeat(nibble).take(40).collect()
    }

    fn sample_v1alpha2(sk: &SigningKey) -> TaskEnvelope {
        let mut env = sample_envelope(sk);
        env.api_version = API_VERSION_V1ALPHA2.into();
        env.repository = Some(RepositoryAuthority {
            identity: "github.com/kotro-labs/kotro-proxy-engine".into(),
            source_pin: sha40('a'),
            base_ref: "refs/heads/main".into(),
            base_sha: sha40('b'),
        });
        env.land = Some(LandAuthority {
            mode: LandMode::DraftPr,
        });
        sign_envelope(&mut env, sk).unwrap();
        env
    }

    #[test]
    fn v1alpha2_root_verifies_with_repository_and_land() {
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_v1alpha2(&sk);
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert!(verify(&env, &ctx).is_ok());
        assert_eq!(
            signing_domain_for(API_VERSION_V1ALPHA2).unwrap(),
            SIGNING_DOMAIN_V1ALPHA2
        );
    }

    #[test]
    fn v1alpha2_rejects_truncated_sha() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_v1alpha2(&sk);
        env.repository.as_mut().unwrap().base_sha = "abc1234".into();
        sign_envelope(&mut env, &sk).unwrap();
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert_eq!(
            verify(&env, &ctx).unwrap_err(),
            TaskReason::TaskSchemaInvalid
        );
    }

    #[test]
    fn v1alpha2_child_cannot_change_base_sha() {
        let sk = SigningKey::generate(&mut OsRng);
        let parent = sample_v1alpha2(&sk);
        let mut child = parent.clone();
        child.task_id = "task-child".into();
        child.depth = 1;
        child.parent = Some(ParentRef {
            task_id: parent.task_id.clone(),
            digest: envelope_digest(&parent).unwrap(),
        });
        child.repository.as_mut().unwrap().base_sha = sha40('c');
        assert_eq!(
            check_non_expansion(&parent, &child).unwrap_err(),
            TaskReason::TaskCapabilityExpansion
        );
    }

    #[test]
    fn v1alpha2_child_may_narrow_land_to_apply_only() {
        let sk = SigningKey::generate(&mut OsRng);
        let parent = sample_v1alpha2(&sk);
        let mut child = parent.clone();
        child.depth = 1;
        child.land.as_mut().unwrap().mode = LandMode::ApplyOnly;
        assert!(check_non_expansion(&parent, &child).is_ok());
    }

    #[test]
    fn v1alpha2_child_cannot_widen_land_to_draft_pr() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut parent = sample_v1alpha2(&sk);
        parent.land.as_mut().unwrap().mode = LandMode::ApplyOnly;
        sign_envelope(&mut parent, &sk).unwrap();
        let mut child = parent.clone();
        child.depth = 1;
        child.land.as_mut().unwrap().mode = LandMode::DraftPr;
        assert_eq!(
            check_non_expansion(&parent, &child).unwrap_err(),
            TaskReason::TaskCapabilityExpansion
        );
    }

    #[test]
    fn v1alpha1_rejects_repository_fields() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut env = sample_envelope(&sk);
        env.repository = Some(RepositoryAuthority {
            identity: "github.com/kotro-labs/x".into(),
            source_pin: sha40('a'),
            base_ref: "main".into(),
            base_sha: sha40('b'),
        });
        sign_envelope(&mut env, &sk).unwrap();
        let trust = trust_for(&sk);
        let parents = MemoryParentStore::default();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: "2026-08-01T18:30:00Z",
            expected_audience: Some("kotro://deployment/acme"),
            kill_engaged: false,
        };
        assert_eq!(
            verify(&env, &ctx).unwrap_err(),
            TaskReason::TaskSchemaInvalid
        );
    }

    #[test]
    fn v1alpha2_uses_distinct_signing_domain_from_v1alpha1() {
        let sk = SigningKey::generate(&mut OsRng);
        let a1 = sample_envelope(&sk);
        let a2 = sample_v1alpha2(&sk);
        let i1 = signing_input(&a1).unwrap();
        let i2 = signing_input(&a2).unwrap();
        assert!(i1.starts_with(SIGNING_DOMAIN_V1ALPHA1));
        assert!(i2.starts_with(SIGNING_DOMAIN_V1ALPHA2));
        assert_ne!(SIGNING_DOMAIN_V1ALPHA1, SIGNING_DOMAIN_V1ALPHA2);
    }
}
