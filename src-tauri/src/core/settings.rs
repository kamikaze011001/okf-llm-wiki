use serde::{Deserialize, Serialize};

fn default_embed_provider() -> String {
    "hash".into()
}
fn default_embed_model() -> String {
    "nomic-embed-text".into()
}
fn default_ollama_url() -> String {
    "http://localhost:11434".into()
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub provider: String, // "claude" | "openai" | "ollama"
    pub model: String,
    pub api_key: String,
    pub wiki_path: String,
    #[serde(default = "default_embed_provider")]
    pub embed_provider: String,
    #[serde(default = "default_embed_model")]
    pub embed_model: String,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
}

// Hand-written so the API key is never emitted via `{:?}` (logs, traces, panics).
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &"[REDACTED]")
            .field("wiki_path", &self.wiki_path)
            .field("embed_provider", &self.embed_provider)
            .field("embed_model", &self.embed_model)
            .field("ollama_url", &self.ollama_url)
            .finish()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            model: "claude-opus-4-8".into(),
            api_key: String::new(),
            wiki_path: String::new(),
            embed_provider: default_embed_provider(),
            embed_model: default_embed_model(),
            ollama_url: default_ollama_url(),
        }
    }
}

use crate::core::provider::{claude::ClaudeProvider, LlmProvider};
use anyhow::{anyhow, Result};
use std::sync::Arc;

pub fn make_provider(s: &Settings) -> Result<Arc<dyn LlmProvider>> {
    match s.provider.as_str() {
        "claude" => Ok(Arc::new(ClaudeProvider::new(
            s.api_key.clone(),
            s.model.clone(),
        ))),
        other => Err(anyhow!("provider '{other}' not yet supported in v1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_json() {
        let s = Settings {
            provider: "claude".into(),
            model: "m".into(),
            api_key: "k".into(),
            wiki_path: "/w".into(),
            ..Settings::default()
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&j).unwrap(), s);
    }

    #[test]
    fn debug_redacts_api_key() {
        let s = Settings {
            api_key: "sk-super-secret".into(),
            ..Settings::default()
        };
        let dbg = format!("{s:?}");
        assert!(
            !dbg.contains("sk-super-secret"),
            "api_key must not appear in Debug output: {dbg}"
        );
        assert!(dbg.contains("[REDACTED]"));
    }

    #[test]
    fn defaults_to_offline_hash_embedder() {
        let s = Settings::default();
        assert_eq!(s.embed_provider, "hash");
        assert_eq!(s.embed_model, "nomic-embed-text");
        assert_eq!(s.ollama_url, "http://localhost:11434");
    }

    #[test]
    fn embed_fields_survive_json_roundtrip() {
        let s = Settings {
            embed_provider: "ollama".into(),
            embed_model: "nomic-embed-text".into(),
            ollama_url: "http://localhost:11434".into(),
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.embed_provider, "ollama");
        assert_eq!(back.embed_model, "nomic-embed-text");
        assert_eq!(back.ollama_url, "http://localhost:11434");
    }
}
