use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub provider: String, // "claude" | "openai" | "ollama"
    pub model: String,
    pub api_key: String,
    pub wiki_path: String,
}

// Hand-written so the API key is never emitted via `{:?}` (logs, traces, panics).
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &"[REDACTED]")
            .field("wiki_path", &self.wiki_path)
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
}
