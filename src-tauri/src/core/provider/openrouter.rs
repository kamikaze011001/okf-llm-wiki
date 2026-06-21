use super::LlmProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Serialize;
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

/// A single model offered by OpenRouter. `name` falls back to `id` when the API omits it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// Pure parse of the `/models` response shape: `{ "data": [ { "id", "name"? }, … ] }`.
/// Entries without an `id` are skipped; a missing `name` defaults to the `id`.
pub fn parse_models(v: &Value) -> Vec<ModelInfo> {
    let Some(arr) = v["data"].as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let name = m["name"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| id.clone());
            Some(ModelInfo { id, name })
        })
        .collect()
}

/// Fetch the (keyless) public model catalog from OpenRouter.
pub async fn fetch_models(client: &reqwest::Client) -> Result<Vec<ModelInfo>> {
    let resp = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "OpenRouter models error {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let v: Value = resp.json().await?;
    Ok(parse_models(&v))
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

    #[test]
    fn parses_models_with_name_fallback_and_skips_missing_id() {
        let v = serde_json::json!({
            "data": [
                { "id": "openai/gpt-4o", "name": "GPT-4o" },
                { "id": "meta/llama-3" },          // no name -> falls back to id
                { "name": "ghost" }                // no id -> skipped
            ]
        });
        let models = parse_models(&v);
        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0],
            ModelInfo {
                id: "openai/gpt-4o".into(),
                name: "GPT-4o".into()
            }
        );
        assert_eq!(
            models[1],
            ModelInfo {
                id: "meta/llama-3".into(),
                name: "meta/llama-3".into()
            }
        );
    }

    #[test]
    fn parses_empty_when_no_data_array() {
        assert!(parse_models(&serde_json::json!({})).is_empty());
    }
}
