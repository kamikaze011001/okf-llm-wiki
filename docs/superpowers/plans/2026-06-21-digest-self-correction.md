# Digest Self-Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded evaluator-optimizer retry loop to the `digest` flow so malformed LLM output (unparseable JSON or blank title/body) is fed back to the model and retried, up to 3 attempts, instead of failing on the first bad reply.

**Architecture:** A single bounded loop lives inside `digest()` in `src-tauri/src/core/digest.rs`. A new `evaluate()` function parses + validates a raw reply into a `DigestJson` or a `DigestFailure`; a new `repair_user_prompt()` builds a correction-flavored user message from the previous raw reply and failure. The `LlmProvider` trait, `commands.rs`, `state.rs`, and the frontend are untouched. A new test-only `ScriptedProvider` (added alongside `FakeProvider`) returns a sequence of replies so the loop can be unit-tested offline.

**Tech Stack:** Rust, Tokio (`#[tokio::test]`), `serde_json`, `anyhow`, `async_trait`. Tests run via `cargo test` from `src-tauri/`.

---

## Important context for the implementer

- **Run all cargo commands from the `src-tauri/` directory.** `cargo` lives at `$HOME/.cargo/bin/cargo` and may not be on `PATH`; if `cargo` is not found, invoke it as `$HOME/.cargo/bin/cargo`.
- **`core/` must stay Tauri-free** — do not import any `tauri::` symbols in `digest.rs` or `fake.rs`.
- **`provider/` is a security-gated area.** This plan only adds a test-only double there (no key handling, no network), but a `security-reviewer` pass is required before the final merge.
- After each task, keep `cargo clippy --all-targets` warning-free and run `cargo fmt` before committing.
- Existing `digest.rs` tests and the existing `FakeProvider` struct literal usages MUST keep working unchanged.

### Relevant existing code

`src-tauri/src/core/digest.rs` today (the parts you will change):

```rust
// JSON contract the LLM must return.
#[derive(Deserialize)]
struct DigestJson {
    title: String,
    description: String,
    tags: Vec<String>,
    body: String,
}

pub async fn digest(
    provider: &dyn LlmProvider,
    source_text: &str,
    resource: Option<&str>,
    note: Option<&str>,
    existing: &[ConceptRef],
) -> Result<DigestResult> {
    let system = build_system_prompt(existing);
    let user = format!(
        "SOURCE:\n{source_text}\n\nUSER NOTE: {}",
        note.unwrap_or("")
    );
    let raw = provider.complete(&system, &user).await?;
    let parsed: DigestJson = serde_json::from_str(extract_json(&raw))
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
    // ... slugify, validate_links, build Page ...
}
```

`extract_json(&str) -> &str` and `first_json_object(&str) -> Option<&str>` already exist and are reused unchanged.

`src-tauri/src/core/provider/fake.rs` today:

```rust
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
```

The `LlmProvider` trait (do not change it):

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String>;
}
```

---

## Task 1: `ScriptedProvider` test double

Add a test-only provider that returns a queued sequence of replies, so the retry loop can be driven deterministically and the number of calls asserted. `FakeProvider` is left exactly as-is.

**Files:**
- Modify: `src-tauri/src/core/provider/fake.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to the existing `#[cfg(test)] mod tests` block in `src-tauri/src/core/provider/fake.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test --lib provider::fake`
Expected: FAIL — `cannot find type ScriptedProvider in this scope` (compile error).

- [ ] **Step 3: Implement `ScriptedProvider`**

Add to `src-tauri/src/core/provider/fake.rs`, after the `FakeProvider` impl block and before the `#[cfg(test)]` module:

```rust
use anyhow::anyhow;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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
```

Note: the existing top-of-file `use anyhow::Result;` stays. The added `use anyhow::anyhow;` is a separate import — keep both. Do not hold the `Mutex` lock across an `.await`; there is no `.await` inside the lock scope here, so this is fine.

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test --lib provider::fake`
Expected: PASS — all `provider::fake` tests green (the existing `fake_completes` plus the two new ones).

- [ ] **Step 5: Lint and format**

Run (from `src-tauri/`): `cargo clippy --all-targets` then `cargo fmt`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/provider/fake.rs
git commit -m "$(cat <<'EOF'
test: add ScriptedProvider for sequenced LLM replies

A test-only LlmProvider that returns queued replies in order and counts
calls, so the upcoming digest retry loop can be driven deterministically.
FakeProvider is unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `DigestFailure` enum + `evaluate()`

Extract the parse-and-validate step into a pure function returning either a parsed `DigestJson` or a structured failure. This is the "evaluator" half of the loop.

**Files:**
- Modify: `src-tauri/src/core/digest.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to the `#[cfg(test)] mod tests` block in `src-tauri/src/core/digest.rs`:

```rust
    #[test]
    fn evaluate_accepts_valid_json() {
        let raw = r#"{"title":"T","description":"d","tags":["a"],"body":"b"}"#;
        let parsed = evaluate(raw).unwrap();
        assert_eq!(parsed.title, "T");
        assert_eq!(parsed.body, "b");
    }

    #[test]
    fn evaluate_rejects_unparseable() {
        let f = evaluate("not json").unwrap_err();
        assert!(matches!(f, DigestFailure::Unparseable(_)));
    }

    #[test]
    fn evaluate_rejects_blank_title() {
        let raw = r#"{"title":"   ","description":"d","tags":[],"body":"b"}"#;
        let f = evaluate(raw).unwrap_err();
        assert!(matches!(f, DigestFailure::EmptyField("title")));
    }

    #[test]
    fn evaluate_rejects_blank_body() {
        let raw = r#"{"title":"T","description":"d","tags":[],"body":""}"#;
        let f = evaluate(raw).unwrap_err();
        assert!(matches!(f, DigestFailure::EmptyField("body")));
    }

    #[test]
    fn digest_failure_display_mentions_field() {
        let f = DigestFailure::EmptyField("title");
        assert!(format!("{f}").contains("title"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test --lib digest::tests::evaluate`
Expected: FAIL — `cannot find function evaluate` / `cannot find type DigestFailure` (compile error).

- [ ] **Step 3: Implement `DigestFailure` and `evaluate`**

In `src-tauri/src/core/digest.rs`, add after the `DigestJson` struct definition (around line 16):

```rust
/// Why a raw LLM reply could not be turned into a usable digest.
enum DigestFailure {
    /// The reply did not contain parseable digest JSON; carries the parser error.
    Unparseable(String),
    /// Parsed, but a required field was blank after trimming.
    EmptyField(&'static str),
}

impl std::fmt::Display for DigestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DigestFailure::Unparseable(e) => write!(f, "reply was not valid digest JSON: {e}"),
            DigestFailure::EmptyField(field) => write!(f, "the \"{field}\" field was empty"),
        }
    }
}

/// Parse + validate a raw LLM reply into a `DigestJson`, or report why it failed.
/// Reuses `extract_json` to peel ```fences```/prose, then requires non-empty
/// `title` and `body` after trimming.
fn evaluate(raw: &str) -> std::result::Result<DigestJson, DigestFailure> {
    let parsed: DigestJson = serde_json::from_str(extract_json(raw))
        .map_err(|e| DigestFailure::Unparseable(e.to_string()))?;
    if parsed.title.trim().is_empty() {
        return Err(DigestFailure::EmptyField("title"));
    }
    if parsed.body.trim().is_empty() {
        return Err(DigestFailure::EmptyField("body"));
    }
    Ok(parsed)
}
```

Note: `evaluate` returns `std::result::Result` explicitly because the file's `use anyhow::Result` shadows `Result` to the single-type-argument alias. Do not change the `use anyhow::{anyhow, Result}` import.

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test --lib digest::tests::evaluate` then `cargo test --lib digest::tests::digest_failure_display_mentions_field`
Expected: PASS — all five new tests green.

- [ ] **Step 5: Lint and format**

Run (from `src-tauri/`): `cargo clippy --all-targets` then `cargo fmt`
Expected: no warnings. (`DigestFailure` and `evaluate` are used by tests, so no dead-code warning.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/digest.rs
git commit -m "$(cat <<'EOF'
feat: add evaluate() and DigestFailure for digest validation

Pure parse-and-validate step: returns a DigestJson or a structured
failure (unparseable JSON, or blank title/body). The evaluator half of
the upcoming self-correction loop. Not yet wired into digest().

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `repair_user_prompt()`

Build the correction-flavored user message used on retries: it states the exact failure, includes the previous raw reply, restates the JSON requirement, and re-appends the original user message.

**Files:**
- Modify: `src-tauri/src/core/digest.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to the `#[cfg(test)] mod tests` block in `src-tauri/src/core/digest.rs`:

```rust
    #[test]
    fn repair_prompt_includes_failure_raw_and_base() {
        let base = "SOURCE:\nhello\n\nUSER NOTE: ";
        let prompt = repair_user_prompt(base, "garbage reply", &DigestFailure::EmptyField("title"));
        assert!(prompt.contains("title"));          // the failure
        assert!(prompt.contains("garbage reply"));  // the previous raw reply
        assert!(prompt.contains("SOURCE:\nhello")); // the original message
        assert!(prompt.contains("non-empty"));      // the restated requirement
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `src-tauri/`): `cargo test --lib digest::tests::repair_prompt_includes_failure_raw_and_base`
Expected: FAIL — `cannot find function repair_user_prompt` (compile error).

- [ ] **Step 3: Implement `repair_user_prompt`**

In `src-tauri/src/core/digest.rs`, add after the `evaluate` function:

```rust
/// Build the retry user message: name the failure, echo the previous reply,
/// restate the JSON requirement, then re-append the original user message so the
/// model still has the source material.
fn repair_user_prompt(base_user: &str, prev_raw: &str, failure: &DigestFailure) -> String {
    format!(
        "Your previous reply could not be used: {failure}.\n\n\
         Previous reply:\n{prev_raw}\n\n\
         Respond ONLY with valid JSON {{\"title\":..,\"description\":..,\"tags\":[..],\"body\":..}}. \
         Both \"title\" and \"body\" must be non-empty.\n\n\
         {base_user}"
    )
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run (from `src-tauri/`): `cargo test --lib digest::tests::repair_prompt_includes_failure_raw_and_base`
Expected: PASS.

- [ ] **Step 5: Lint and format**

Run (from `src-tauri/`): `cargo clippy --all-targets` then `cargo fmt`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/digest.rs
git commit -m "$(cat <<'EOF'
feat: add repair_user_prompt() for digest retries

Builds the correction-flavored user message: states the failure, echoes
the previous reply, restates the JSON requirement, and re-appends the
original source. Not yet wired into digest().

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Wire the retry loop into `digest()`

Replace the single call-and-parse with a bounded loop that calls `complete`, runs `evaluate`, and on failure retries with `repair_user_prompt`, up to 3 attempts. Transport errors propagate immediately. The rest of `digest()` (slugify, link validation, page build) is unchanged.

**Files:**
- Modify: `src-tauri/src/core/digest.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to the `#[cfg(test)] mod tests` block in `src-tauri/src/core/digest.rs`. Add `use crate::core::provider::fake::ScriptedProvider;` to the test module's imports (next to the existing `use crate::core::provider::fake::FakeProvider;`).

```rust
    #[tokio::test]
    async fn retries_after_unparseable_then_succeeds() {
        let good = r#"{"title":"T","description":"d","tags":[],"body":"**TL;DR.** x"}"#;
        let p = ScriptedProvider::new(vec!["not json".into(), good.into()]);
        let r = digest(&p, "src", None, None, &[]).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("T".into()));
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn retries_after_empty_title_then_succeeds() {
        let blank = r#"{"title":"  ","description":"d","tags":[],"body":"b"}"#;
        let good = r#"{"title":"T","description":"d","tags":[],"body":"b"}"#;
        let p = ScriptedProvider::new(vec![blank.into(), good.into()]);
        let r = digest(&p, "src", None, None, &[]).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("T".into()));
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn fails_after_three_bad_attempts() {
        let p = ScriptedProvider::new(vec!["bad1".into(), "bad2".into(), "bad3".into()]);
        let err = digest(&p, "src", None, None, &[]).await.unwrap_err();
        assert!(format!("{err}").contains("3 attempts"));
        assert_eq!(p.calls(), 3);
    }

    #[tokio::test]
    async fn succeeds_on_first_attempt_without_retry() {
        let good = r#"{"title":"T","description":"d","tags":[],"body":"b"}"#;
        let p = ScriptedProvider::new(vec![good.into()]);
        let r = digest(&p, "src", None, None, &[]).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("T".into()));
        assert_eq!(p.calls(), 1);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test --lib digest::tests::retries digest::tests::fails_after digest::tests::succeeds_on_first`
Expected: FAIL — `fails_after_three_bad_attempts` fails because the current code errors on attempt 1 (`p.calls() == 1`, not 3) and the message lacks "3 attempts"; the retry tests fail because attempt 1's bad reply is surfaced immediately.

- [ ] **Step 3: Replace the call-and-parse block with the retry loop**

This has two edits in `src-tauri/src/core/digest.rs`: (a) swap the inline call-and-parse for a call to a new helper, and (b) add the helper function.

**(a)** In `digest()`, replace these two lines:

```rust
    let raw = provider.complete(&system, &user).await?;
    let parsed: DigestJson = serde_json::from_str(extract_json(&raw))
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
```

with:

```rust
    const MAX_ATTEMPTS: usize = 3;
    let parsed = run_digest_attempts(provider, &system, &user, MAX_ATTEMPTS).await?;
```

The line above it (`let user = format!(...)`) stays. Everything below it (`let slug = slugify(&parsed.title);` onward) stays exactly as it is.

**(b)** Add this free function near `evaluate` (e.g. directly after `repair_user_prompt`):

```rust
/// Call the provider up to `max_attempts` times, feeding each failure back as a
/// repair prompt, and return the first reply that passes `evaluate`. Transport
/// errors propagate immediately. Returns an error naming the attempt count if
/// every attempt fails validation.
async fn run_digest_attempts(
    provider: &dyn LlmProvider,
    system: &str,
    base_user: &str,
    max_attempts: usize,
) -> Result<DigestJson> {
    let mut last: Option<(String, DigestFailure)> = None;
    for _ in 1..=max_attempts {
        let user = match &last {
            None => base_user.to_string(),
            Some((prev_raw, failure)) => repair_user_prompt(base_user, prev_raw, failure),
        };
        let raw = provider.complete(system, &user).await?;
        match evaluate(&raw) {
            Ok(parsed) => return Ok(parsed),
            Err(failure) => last = Some((raw, failure)),
        }
    }
    // Loop exhausted without success: report the last failure.
    let failure = last.expect("at least one attempt ran").1;
    Err(anyhow!("digest failed after {max_attempts} attempts: {failure}"))
}
```

Notes for the implementer:
- The loop counter is unused, hence `for _ in 1..=max_attempts`. The retry/initial distinction is driven by whether `last` is `Some`, not by the index.
- `anyhow` is still used (inside this helper), so the `use anyhow::{anyhow, Result};` import stays. `serde_json` is now only referenced inside `evaluate`, which still uses it — the import stays.
- `provider` is `&dyn LlmProvider`; pass it straight through (it is already a trait object reference in `digest()`'s signature).

- [ ] **Step 4: Run the new tests to verify they pass**

Run (from `src-tauri/`): `cargo test --lib digest::tests::retries digest::tests::fails_after digest::tests::succeeds_on_first`
Expected: PASS — all four new loop tests green.

- [ ] **Step 5: Run the full digest module + workspace tests**

Run (from `src-tauri/`): `cargo test --lib digest`
Expected: PASS — the new tests plus all pre-existing digest tests (`produces_concept_page_from_llm_json`, `errors_on_malformed_json`, `parses_json_wrapped_in_code_fence`, `parses_json_with_surrounding_prose`, `drops_hallucinated_links_keeps_valid_ones`, etc.). Note `errors_on_malformed_json` still passes: a single `"not json"` reply now triggers 3 attempts (the `FakeProvider` returns the same bad reply each time) and `digest` still returns `Err`.

Then run the whole suite: `cargo test`
Expected: PASS — entire workspace green.

- [ ] **Step 6: Lint and format**

Run (from `src-tauri/`): `cargo clippy --all-targets` then `cargo fmt`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/core/digest.rs
git commit -m "$(cat <<'EOF'
feat: self-correcting digest with bounded retry loop

digest() now retries on unparseable JSON or blank title/body, feeding
the failure back to the model via a repair prompt, up to 3 attempts.
Transport errors still propagate immediately. Exhaustion returns an
error naming the attempt count.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Final steps (after all tasks)

- [ ] **Security review:** run the `security-reviewer` agent over the diff (touches `provider/fake.rs`, a gated area). Expected: SAFE — the change is a test-only double with no key handling or network.
- [ ] **Final full-suite check** (from `src-tauri/`): `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Frontend is untouched, but optionally confirm `npm run build` still passes.
- [ ] Use `superpowers:finishing-a-development-branch` to wrap up (push + PR is user-triggered; do not auto-push).

---

## Notes / decisions captured from the spec

- **3 attempts** = 1 initial + 2 retries (`MAX_ATTEMPTS = 3`).
- **Failure criteria:** unparseable JSON OR blank `title`/`body` after trim.
- **Repair mechanism:** resend previous raw reply + exact failure + restated requirement + original source.
- **Transport errors are not retried** — they propagate via `?`.
- **No `LlmProvider` trait change.** No `commands.rs`/`state.rs`/frontend change.
- **`FakeProvider` unchanged**; new `ScriptedProvider` drives the loop tests.
