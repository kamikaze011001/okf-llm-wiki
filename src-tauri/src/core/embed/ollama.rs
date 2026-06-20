use super::Embedder;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Embeds text via a local Ollama server's `/api/embeddings` endpoint.
///
/// Targets keyless localhost (no auth header). Any non-2xx response or transport
/// error surfaces as `Err` so the UI can tell the user Ollama is unreachable.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Vec<f32>,
}

impl OllamaEmbedder {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&EmbedRequest {
                model: &self.model,
                prompt: text,
            })
            .send()
            .await
            .context("requesting embedding from Ollama")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama API error {status}: {body}"));
        }
        let parsed: EmbedResponse = resp
            .json()
            .await
            .context("parsing Ollama embedding response")?;
        if parsed.embedding.is_empty() {
            return Err(anyhow!(
                "Ollama returned an empty embedding for model '{}'",
                self.model
            ));
        }
        Ok(parsed.embedding)
    }

    fn id(&self) -> String {
        format!("ollama:{}", self.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_encodes_model() {
        let e = OllamaEmbedder::new("http://localhost:11434".into(), "nomic-embed-text".into());
        assert_eq!(e.id(), "ollama:nomic-embed-text");
    }
}
