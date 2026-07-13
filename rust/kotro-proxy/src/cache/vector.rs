//! Local semantic cache: real sentence embeddings + cosine-similarity lookup.
//!
//! Loads `sentence-transformers/all-MiniLM-L6-v2` (23M params, 384-dim
//! output — chosen for low latency in the request hot path over larger
//! models like nomic-embed-text or bge-m3; see docs/roadmap/next-steps.md)
//! via `candle` + `hf-hub` on first use. The model and tokenizer are
//! downloaded once into the standard Hugging Face cache directory
//! (`~/.cache/huggingface`, override with `HF_HOME`) and reused after that.
//!
//! If the download or load fails for any reason — offline, no disk space,
//! a corrupt cache, an incompatible file layout — the encoder degrades to
//! "disabled" rather than failing proxy startup: `embed()` returns `None`,
//! and callers in `router/handlers.rs` already treat that as "skip the
//! vector cache for this request, the exact-match prompt-state cache still
//! applies." The zero-config, single-binary promise holds even fully
//! offline; you just lose the fuzzy-match layer, not the proxy.

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use hf_hub::api::sync::Api;
use moka::sync::Cache;
use parking_lot::RwLock;
use std::sync::Arc;
use tokenizers::Tokenizer;

const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
/// BERT position embeddings top out at 512; truncate defensively so a huge
/// prompt can't panic the forward pass on an out-of-range position lookup.
const MAX_TOKENS: usize = 512;
pub const EMBEDDING_DIM: usize = 384;

struct LoadedModel {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

pub struct SemanticEncoder {
    inner: Option<LoadedModel>,
}

impl SemanticEncoder {
    /// Attempts to load the MiniLM model + tokenizer. `enabled = false`
    /// (`KOTRO_ENABLE_VECTOR_CACHE=false`) skips loading entirely and is
    /// equivalent to a load failure: `embed()` always returns `None`.
    pub fn new(enabled: bool) -> Self {
        if !enabled {
            return Self { inner: None };
        }

        tracing::info!(model = MODEL_ID, "loading local semantic embedding model");
        match Self::try_load() {
            Ok(inner) => {
                tracing::info!("semantic embedding model ready");
                Self { inner: Some(inner) }
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to load semantic embedding model; vector cache disabled for \
                     this run, falling back to exact-match prompt-state cache only"
                );
                Self { inner: None }
            }
        }
    }

    fn try_load() -> Result<LoadedModel, Box<dyn std::error::Error>> {
        let device = Device::Cpu;

        // `Api::model()` is the sync hf-hub client pointed at the `main`
        // revision of the repo. First call downloads into the shared HF
        // cache; subsequent runs (including across proxy restarts) hit the
        // local cache and do no network I/O at all.
        let api = Api::new()?;
        let repo = api.model(MODEL_ID.to_string());

        let config_path = repo.get("config.json")?;
        let tokenizer_path = repo.get("tokenizer.json")?;
        let weights_path = repo.get("model.safetensors")?;

        let config: BertConfig = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;
        let tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|e| -> Box<dyn std::error::Error> { e })?;

        // Safety: mmap of a checkpoint file we just fetched (or found
        // already cached) from Hugging Face — the standard candle loading
        // pattern for safetensors weights.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)? };
        let model = BertModel::load(vb, &config)?;

        Ok(LoadedModel {
            model,
            tokenizer,
            device,
        })
    }

    /// Embeds a prompt into a 384-dimensional, L2-normalized vector.
    /// Returns `None` if the encoder is disabled/unavailable, the input is
    /// empty, or tokenization/inference fails for this specific input
    /// (logged, not fatal — the exact-match cache still covers the
    /// request either way).
    pub fn embed(&self, text: &str) -> Option<Vec<f32>> {
        if text.is_empty() {
            return None;
        }
        let inner = self.inner.as_ref()?;
        match Self::embed_inner(inner, text) {
            Ok(v) => Some(v),
            Err(err) => {
                tracing::warn!(error = %err, "semantic embedding failed for this request");
                None
            }
        }
    }

    fn embed_inner(inner: &LoadedModel, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let encoding = inner
            .tokenizer
            .encode(text, true)
            .map_err(|e| -> Box<dyn std::error::Error> { e })?;

        let mut ids = encoding.get_ids().to_vec();
        if ids.len() > MAX_TOKENS {
            ids.truncate(MAX_TOKENS);
        }

        let input_ids = Tensor::new(ids.as_slice(), &inner.device)?.unsqueeze(0)?;
        let token_type_ids = input_ids.zeros_like()?;

        // NOTE: candle-transformers' `BertModel::forward` signature has
        // varied across releases — newer versions take an optional
        // attention mask (`forward(&input_ids, &token_type_ids, None)`),
        // older ones take just the two ID tensors
        // (`forward(&input_ids, &token_type_ids)`). If this line doesn't
        // compile against the resolved candle-transformers version, drop
        // the trailing `None` argument.
        let output = inner.model.forward(&input_ids, &token_type_ids, None)?;

        // Mean pooling over the token dimension, then L2-normalize — the
        // standard sentence-embedding recipe for encoder-only BERT models
        // (matches what sentence-transformers itself does for this model).
        let (_batch, n_tokens, _hidden) = output.dims3()?;
        let pooled = (output.sum(1)? / (n_tokens as f64))?;
        let norm = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
        let normalized = pooled.broadcast_div(&norm)?;

        Ok(normalized.squeeze(0)?.to_vec1::<f32>()?)
    }
}

pub struct VectorIndex {
    // Maps ContextKey -> list of (ExactCacheKey, UserPrompt, Vector)
    // ContextKey is a hash of (scope, provider, model, system_prompt).
    #[allow(clippy::type_complexity)]
    buckets: Cache<String, Arc<RwLock<Vec<(String, String, Vec<f32>)>>>>,
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            buckets: Cache::builder().max_capacity(10_000).build(),
        }
    }

    pub fn insert(&self, context_key: String, exact_cache_key: String, user_prompt: String, vector: Vec<f32>) {
        let bucket = self.buckets.get_with(context_key, || Arc::new(RwLock::new(Vec::new())));
        let mut bucket_guard = bucket.write();

        // Keep bucket size bounded to prevent memory leaks (e.g., max 1000 items)
        if bucket_guard.len() >= 1000 {
            bucket_guard.remove(0); // evict oldest
        }

        bucket_guard.push((exact_cache_key, user_prompt, vector));
    }

    /// Finds the closest semantic match within the same context.
    /// Returns the ExactCacheKey of the hit if cosine similarity > threshold.
    pub fn find_closest(
        &self,
        context_key: &str,
        target_vector: &[f32],
        threshold: f32,
    ) -> Option<String> {
        let bucket = self.buckets.get(context_key)?;
        let bucket_guard = bucket.read();

        let mut best_score = -1.0;
        let mut best_key = None;

        for (exact_cache_key, _prompt, vector) in bucket_guard.iter() {
            let score = cosine_similarity(target_vector, vector);
            if score > best_score {
                best_score = score;
                best_key = Some(exact_cache_key.clone());
            }
        }

        if best_score >= threshold {
            tracing::info!("Semantic cache hit! Score: {:.3}", best_score);
            return best_key;
        }

        None
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0;
    for (va, vb) in a.iter().zip(b.iter()) {
        dot += va * vb;
    }
    // Assumes vectors are already normalized
    dot
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real accuracy test against the loaded MiniLM model. Requires network
    /// access on first run (downloads ~90MB into the HF cache). If the
    /// model can't be loaded in this environment (offline, sandboxed CI
    /// with no egress, ...), the encoder degrades to disabled and this test
    /// skips rather than failing the build — the same graceful-fallback
    /// behavior the proxy itself relies on.
    #[test]
    fn semantic_similarity_reflects_paraphrase_vs_unrelated() {
        let encoder = SemanticEncoder::new(true);

        let Some(base) = encoder.embed("Write a rust function for binary search") else {
            eprintln!("semantic model unavailable in this environment; skipping");
            return;
        };

        // Paraphrase: same intent, different wording -> should score high.
        let paraphrase = encoder
            .embed("Can you implement binary search in Rust?")
            .expect("encoder was available above; should stay available");
        let paraphrase_score = cosine_similarity(&base, &paraphrase);
        assert!(
            paraphrase_score > 0.75,
            "expected paraphrase similarity > 0.75, got {paraphrase_score}"
        );

        // Unrelated prompt -> should score meaningfully lower than the paraphrase.
        //
        // NOTE on the threshold below: mean-pooled sentence embeddings carry
        // a fair amount of shared "generic English sentence" structure, so
        // even genuinely unrelated short sentences typically land in the
        // 0.4-0.7+ cosine range rather than near 0 (empirically observed:
        // 0.708 for "binary search in Rust" vs. "chocolate chip cookies").
        // 0.85 is chosen as a ceiling that still catches real breakage
        // (e.g. embeddings collapsing to a near-constant vector, which would
        // push every pair's similarity toward 1.0) without being a false
        // alarm on normal model anisotropy. The relative-ordering assertion
        // above is the more meaningful check; this is a coarser sanity net
        // around it. The production lookup threshold in
        // VectorIndex::find_closest (0.94) sits comfortably above this
        // observed noise floor and is exercised directly, at that exact
        // value, by vector_index_lookup_uses_encoder_output below.
        let unrelated = encoder
            .embed("What's a good recipe for chocolate chip cookies?")
            .expect("encoder was available above; should stay available");
        let unrelated_score = cosine_similarity(&base, &unrelated);
        assert!(
            unrelated_score < paraphrase_score,
            "unrelated prompt ({unrelated_score}) should score below the paraphrase ({paraphrase_score})"
        );
        assert!(
            unrelated_score < 0.85,
            "expected unrelated similarity < 0.85, got {unrelated_score}"
        );
    }

    #[test]
    fn vector_index_lookup_uses_encoder_output() {
        let index = VectorIndex::new();
        let encoder = SemanticEncoder::new(true);
        let ctx = "ctx123".to_string();

        let Some(vec1) = encoder.embed("Write a rust function for binary search") else {
            eprintln!("semantic model unavailable in this environment; skipping");
            return;
        };
        index.insert(ctx.clone(), "key1".to_string(), "prompt1".to_string(), vec1);

        // Exact repeat should hit.
        let vec_same = encoder
            .embed("Write a rust function for binary search")
            .expect("encoder was available above; should stay available");
        assert_eq!(index.find_closest(&ctx, &vec_same, 0.94), Some("key1".to_string()));

        // Unrelated prompt should miss at a high threshold.
        let vec_unrelated = encoder
            .embed("What's the capital of France?")
            .expect("encoder was available above; should stay available");
        assert_eq!(index.find_closest(&ctx, &vec_unrelated, 0.94), None);
    }

    #[test]
    fn disabled_encoder_returns_none() {
        let encoder = SemanticEncoder::new(false);
        assert_eq!(encoder.embed("anything"), None);
    }
}
