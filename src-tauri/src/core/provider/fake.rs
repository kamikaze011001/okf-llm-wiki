use super::LlmProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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

/// Test-only provider that returns a queued sequence of replies, one per
/// `complete` call. Errors when the queue is exhausted so an unexpected extra
/// call fails loudly. `calls()` reports how many times `complete` was invoked.
pub struct ScriptedProvider {
    replies: Mutex<VecDeque<String>>,
    calls: AtomicUsize,
}

impl ScriptedProvider {
    pub fn new(replies: Vec<String>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            calls: AtomicUsize::new(0),
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow!("ScriptedProvider exhausted: no more replies queued"))
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

    #[tokio::test]
    async fn scripted_returns_replies_in_order_and_counts_calls() {
        let p = ScriptedProvider::new(vec!["first".into(), "second".into()]);
        assert_eq!(p.complete("s", "u").await.unwrap(), "first");
        assert_eq!(p.complete("s", "u").await.unwrap(), "second");
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn scripted_errors_when_exhausted() {
        let p = ScriptedProvider::new(vec!["only".into()]);
        assert_eq!(p.complete("s", "u").await.unwrap(), "only");
        assert!(p.complete("s", "u").await.is_err());
        assert_eq!(p.calls(), 2);
    }
}
