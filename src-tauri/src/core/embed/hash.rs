use super::Embedder;
use crate::core::retrieval::hash_embed;
use anyhow::Result;
use async_trait::async_trait;

/// Offline, deterministic fallback embedder over the existing FNV hashing scheme.
pub struct HashEmbedder;

#[async_trait]
impl Embedder for HashEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(hash_embed(text))
    }
    fn id(&self) -> String {
        "hash-fnv-256".to_string()
    }
}
