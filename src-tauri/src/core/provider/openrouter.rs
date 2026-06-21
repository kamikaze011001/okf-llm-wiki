use super::LlmProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct OpenRouterProvider {
    pub api_key: String,
    pub model: String, // e.g. "openai/gpt-4o"
    pub client: reqwest::Client,
}

impl OpenRouterProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
    pub(crate) fn chat_body(&self, system: &str, user: &str) -> Value {
        json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        })
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("X-Title", "okf-llm-wiki")
            .json(&self.chat_body(system, user))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenRouter API error {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let v: Value = resp.json().await?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected OpenRouter response shape"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_chat_body() {
        let p = OpenRouterProvider::new("k".into(), "openai/gpt-4o".into());
        let b = p.chat_body("be brief", "hi");
        assert_eq!(b["model"], "openai/gpt-4o");
        assert_eq!(b["max_tokens"], 4096);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][0]["content"], "be brief");
        assert_eq!(b["messages"][1]["role"], "user");
        assert_eq!(b["messages"][1]["content"], "hi");
    }
}
