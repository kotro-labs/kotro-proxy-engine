use std::collections::HashMap;
use parking_lot::RwLock;

// In a real implementation we would import candle-core and hf-hub to load BERT weights.
// For the sake of this prototype proxy integration, we will stub the actual ML math 
// and simulate the vector generation. (Note: standard candle-transformers integration 
// requires ~200 lines of config parsing and weight mapping which we simplify here).

pub struct SemanticEncoder {
    // model: BertModel,
    // tokenizer: Tokenizer,
    enabled: bool,
}

impl SemanticEncoder {
    pub fn new(enabled: bool) -> Self {
        if enabled {
            tracing::info!("Initializing Vector Semantic Encoder (MiniLM)");
            // Here we would use hf_hub::api::sync::Api::new() to fetch 
            // "sentence-transformers/all-MiniLM-L6-v2" weights and tokenizer.
        }
        Self { enabled }
    }

    /// Embeds a prompt into a 384-dimensional vector.
    pub fn embed(&self, text: &str) -> Option<Vec<f32>> {
        if !self.enabled || text.is_empty() {
            return None;
        }

        // Mock embedding generation: a real implementation would tokenize `text`, 
        // pass it through `self.model.forward()`, and apply mean pooling.
        // For demonstration of the caching logic, we deterministically generate 
        // a 384-d vector from the string length and basic ascii values.
        let mut vec = vec![0.0f32; 384];
        let bytes = text.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            vec[i % 384] += (b as f32) / 255.0;
        }
        
        // Normalize vector
        let mut norm = 0.0;
        for v in &vec {
            norm += v * v;
        }
        let norm = norm.sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        
        Some(vec)
    }
}

pub struct VectorIndex {
    // Maps ContextKey -> list of (ExactCacheKey, UserPrompt, Vector)
    // ContextKey is a hash of (scope, provider, model, system_prompt).
    buckets: RwLock<HashMap<String, Vec<(String, String, Vec<f32>)>>>,
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            buckets: RwLock::new(HashMap::new()),
        }
    }

    pub fn insert(&self, context_key: String, exact_cache_key: String, user_prompt: String, vector: Vec<f32>) {
        let mut buckets = self.buckets.write();
        let bucket = buckets.entry(context_key).or_insert_with(Vec::new);
        
        // Keep bucket size bounded to prevent memory leaks (e.g., max 1000 items)
        if bucket.len() >= 1000 {
            bucket.remove(0); // evict oldest
        }
        
        bucket.push((exact_cache_key, user_prompt, vector));
    }

    /// Finds the closest semantic match within the same context. 
    /// Returns the ExactCacheKey of the hit if cosine similarity > threshold.
    pub fn find_closest(
        &self,
        context_key: &str,
        target_vector: &[f32],
        threshold: f32,
    ) -> Option<String> {
        let buckets = self.buckets.read();
        let bucket = buckets.get(context_key)?;

        let mut best_score = -1.0;
        let mut best_key = None;

        for (exact_cache_key, _prompt, vector) in bucket {
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

    #[test]
    fn test_vector_index_similarity() {
        let index = VectorIndex::new();
        let encoder = SemanticEncoder::new(true);

        let ctx = "ctx123".to_string();
        
        // Let's insert a prompt
        let prompt1 = "Write a rust function for binary search".to_string();
        let vec1 = encoder.embed(&prompt1).unwrap();
        index.insert(ctx.clone(), "key1".to_string(), prompt1, vec1.clone());

        // A highly similar prompt should hit
        let prompt2 = "Write a rust function for binary search".to_string(); // exact same
        let vec2 = encoder.embed(&prompt2).unwrap();
        
        let hit = index.find_closest(&ctx, &vec2, 0.94);
        assert_eq!(hit, Some("key1".to_string()));

        // An entirely different prompt should miss
        let prompt3 = "Z".repeat(100);
        let vec3 = encoder.embed(&prompt3).unwrap();
        let miss = index.find_closest(&ctx, &vec3, 0.94);
        assert_eq!(miss, None);
    }
}
