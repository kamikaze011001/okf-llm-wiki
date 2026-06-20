use super::LlmProvider;
use anyhow::Result;
use async_trait::async_trait;

/// Deterministic provider for tests. `complete` returns `reply`.
pub struct FakeProvider {
    pub reply: String,
}

#[async_trait]
impl LlmProvider for FakeProvider {
    async fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        Ok(self.reply.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fake_completes() {
        let p = FakeProvider { reply: "ok".into() };
        assert_eq!(p.complete("s", "u").await.unwrap(), "ok");
    }
}
