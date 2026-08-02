//! TaskEnvelope v1alpha1 — signed, non-expanding task authority (S4 / C6).

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_VERSION: &str = "kotro.dev/v1alpha1";
pub const KIND: &str = "TaskEnvelope";
pub const SIGNING_DOMAIN: &[u8] = b"KOTRO-TASK-ENVELOPE-V1ALPHA1\0";
pub const HARD_MAX_DEPTH: u32 = 8;
pub const MAX_ENVELOPE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskEnvelope {
    pub api_version: String,
    pub kind: String,
    pub task_id: String,
    pub audience: String,
    pub issuer: String,
    pub principal: EnvelopePrincipal,
    pub agent_scope: AgentScope,
    pub issued_at: String,
    pub not_before: String,
    pub expires_at: String,
    pub nonce: String,
    pub depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentRef>,
    pub capabilities: Capabilities,
    pub delegation: Delegation,
    pub signature: EnvelopeSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvelopePrincipal {
    pub subject: String,
    pub issuer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentScope {
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workload_identities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentRef {
    pub task_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destinations: Vec<DestinationCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<CredentialCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem: Vec<FilesystemCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<Budgets>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolCapability {
    pub server: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_schema_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<ArgumentConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArgumentConstraint {
    Any,
    Exact { hashes: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelCapability {
    pub provider: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DestinationCapability {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialCapability {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilesystemCapability {
    pub root: String,
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    pub max_tool_calls: u64,
    pub max_model_calls: u64,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_microusd: u64,
    pub max_duration_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    pub max_depth: u32,
    pub signers: Vec<DelegationSigner>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DelegationSigner {
    pub key_id: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

/// Unsigned envelope view used for signing input (signature field removed).
pub fn unsigned_value(envelope: &TaskEnvelope) -> Result<Value, String> {
    let mut v = serde_json::to_value(envelope).map_err(|e| e.to_string())?;
    if let Some(obj) = v.as_object_mut() {
        obj.remove("signature");
    }
    Ok(v)
}

pub fn signing_input(envelope: &TaskEnvelope) -> Result<Vec<u8>, String> {
    let unsigned = unsigned_value(envelope)?;
    let jcs = serde_json_canonicalizer::to_vec(&unsigned).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(SIGNING_DOMAIN.len() + jcs.len());
    out.extend_from_slice(SIGNING_DOMAIN);
    out.extend_from_slice(&jcs);
    Ok(out)
}

pub fn envelope_digest(envelope: &TaskEnvelope) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let v = serde_json::to_value(envelope).map_err(|e| e.to_string())?;
    let jcs = serde_json_canonicalizer::to_vec(&v).map_err(|e| e.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(&jcs)))
}

pub fn key_id_for_public_key(raw32: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(raw32);
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, digest);
    format!("ed25519:{b64}")
}
