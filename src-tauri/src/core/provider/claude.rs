use super::LlmProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ClaudeProvider {
    pub api_key: String,
    pub model: String, // e.g. "claude-opus-4-8"
    pub client: reqwest::Client,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
    pub(crate) fn messages_body(&self, system: &str, user: &str) -> Value {
        json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system,
            "messages": [{ "role": "user", "content": user }]
        })
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.messages_body(system, user))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "Claude API error {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let v: Value = resp.json().await?;
        v["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected Claude response shape"))
    }
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Anthropic has no first-party embeddings; v1 uses a local hashing embedder.
        Ok(crate::core::retrieval::hash_embed(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_messages_body() {
        let p = ClaudeProvider::new("k".into(), "claude-opus-4-8".into());
        let b = p.messages_body("be brief", "hi");
        assert_eq!(b["model"], "claude-opus-4-8");
        assert_eq!(b["system"], "be brief");
        assert_eq!(b["messages"][0]["content"], "hi");
    }
}
