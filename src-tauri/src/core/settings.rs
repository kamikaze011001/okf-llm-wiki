use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub provider: String,    // "claude" | "openai" | "ollama"
    pub model: String,
    pub api_key: String,
    pub wiki_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { provider: "claude".into(), model: "claude-opus-4-8".into(),
               api_key: String::new(), wiki_path: String::new() }
    }
}

use crate::core::provider::{LlmProvider, claude::ClaudeProvider};
use std::sync::Arc;
use anyhow::{Result, anyhow};

pub fn make_provider(s: &Settings) -> Result<Arc<dyn LlmProvider>> {
    match s.provider.as_str() {
        "claude" => Ok(Arc::new(ClaudeProvider::new(s.api_key.clone(), s.model.clone()))),
        other => Err(anyhow!("provider '{other}' not yet supported in v1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_json() {
        let s = Settings { provider: "claude".into(), model: "m".into(), api_key: "k".into(), wiki_path: "/w".into() };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&j).unwrap(), s);
    }
}
