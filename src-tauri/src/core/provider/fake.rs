use super::LlmProvider;
use anyhow::Result;
use async_trait::async_trait;

/// Deterministic provider for tests. `complete` returns `reply`;
/// `embed` returns a tiny bag-of-chars vector so similar text scores higher.
pub struct FakeProvider { pub reply: String }

#[async_trait]
impl LlmProvider for FakeProvider {
    async fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        Ok(self.reply.clone())
    }
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0f32; 26];
        for c in text.to_ascii_lowercase().chars() {
            if c.is_ascii_lowercase() { v[(c as u8 - b'a') as usize] += 1.0; }
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fake_completes_and_embeds() {
        let p = FakeProvider { reply: "ok".into() };
        assert_eq!(p.complete("s", "u").await.unwrap(), "ok");
        assert_eq!(p.embed("abc").await.unwrap().len(), 26);
    }
}
