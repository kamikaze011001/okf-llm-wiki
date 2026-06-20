use crate::core::settings::Settings;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;

pub mod hash;
pub mod ollama;

pub use hash::HashEmbedder;
pub use ollama::OllamaEmbedder;

/// Turns text into a dense vector. Async + fallible because real backends do network I/O.
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Stable identifier of this embedder+model. Used to detect when a persisted
    /// index was built by a different embedder and must be fully rebuilt.
    fn id(&self) -> String;
}

/// Construct the embedder selected in `Settings`. Pure construction — no network call.
pub fn make_embedder(s: &Settings) -> Result<Arc<dyn Embedder>> {
    match s.embed_provider.as_str() {
        "hash" => Ok(Arc::new(HashEmbedder)),
        "ollama" => Ok(Arc::new(OllamaEmbedder::new(
            s.ollama_url.clone(),
            s.embed_model.clone(),
        ))),
        other => Err(anyhow!(
            "embed provider '{other}' not supported (use 'hash' or 'ollama')"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::Settings;

    #[tokio::test]
    async fn hash_embedder_is_deterministic_and_normalized() {
        let e = HashEmbedder;
        let a = e.embed("vitamin d sleep").await.unwrap();
        let b = e.embed("vitamin d sleep").await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 256);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4 || norm == 0.0);
        assert_eq!(e.id(), "hash-fnv-256");
    }

    #[test]
    fn make_embedder_selects_hash_by_default() {
        let s = Settings::default();
        let e = make_embedder(&s).unwrap();
        assert_eq!(e.id(), "hash-fnv-256");
    }

    #[test]
    fn make_embedder_rejects_unknown_provider() {
        let s = Settings {
            embed_provider: "nope".into(),
            ..Settings::default()
        };
        assert!(make_embedder(&s).is_err());
    }

    #[test]
    fn make_embedder_selects_ollama() {
        let s = Settings {
            embed_provider: "ollama".into(),
            embed_model: "nomic-embed-text".into(),
            ollama_url: "http://localhost:11434".into(),
            ..Settings::default()
        };
        let e = make_embedder(&s).unwrap();
        assert_eq!(e.id(), "ollama:nomic-embed-text");
    }
}
