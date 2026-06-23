# Ask Groundedness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wrap the `ask` RAG flow in a bounded evaluator-optimizer loop (draft → LLM-as-judge → repair-with-feedback → abstain on exhaustion) and replace the over-claiming citations with the retrieved paths the answer actually cited.

**Architecture:** A loop inside `src-tauri/src/core/ask.rs` mirroring `digest.rs`. Each attempt makes a *draft* LLM call and a *judge* LLM call; the judge returns a JSON verdict (`accept`/`revise`/`abstain`). `accept` returns the draft with deterministically filtered citations; `revise` re-drafts with feedback; `abstain` (or an unparseable verdict, or attempt exhaustion) returns a canonical abstention. Transport errors propagate immediately. The public `Answer { text, citations }` shape is unchanged, so `commands.rs` and the frontend need no changes.

**Tech Stack:** Rust, Tauri 2 core (`src-tauri/src/core/`), `anyhow`, `serde`/`serde_json`, `async_trait`, `tokio` test harness. The `ScriptedProvider` test double already exists in `src-tauri/src/core/provider/fake.rs`.

---

## Reference: existing code this plan builds on

- `src-tauri/src/core/ask.rs` — current one-shot `ask(provider, question, hits) -> Result<Answer>`; `Answer { text, citations }`. The current body returns **all** retrieved paths as citations.
- `src-tauri/src/core/digest.rs:158` — `fn extract_json(raw: &str) -> &str` peels ```` ```fences ```` / prose around a JSON object (calls private `first_json_object`). Task 1 promotes it to `pub(crate)` so the ask flow reuses it.
- `src-tauri/src/core/provider/fake.rs` — `ScriptedProvider::new(Vec<String>)` returns queued replies in order, errors (message contains `"exhausted"`) when the queue is empty, and `calls()` reports the number of `complete()` invocations.
- `src-tauri/src/core/index_store.rs:11` — `Chunk { path: String, chunk_id: usize, text: String, vector: Vec<f32> }`.
- **Convention:** `anyhow::Result` shadows `Result` in these files to a one-type-arg alias, so any function returning the two-type-arg form must spell out `std::result::Result<_, _>` (see `digest.rs::evaluate`).

**Run cargo from `src-tauri/`.** In this environment cargo is at `$HOME/.cargo/bin/cargo`; a plain `cargo` may not be on PATH.

---

### Task 1: Promote `extract_json` to `pub(crate)`

Pure visibility change in `digest.rs` so `ask.rs` can reuse the JSON-extraction helper (DRY). No behavior change; verified by the existing suite staying green.

**Files:**
- Modify: `src-tauri/src/core/digest.rs:158`

- [ ] **Step 1: Change the visibility**

In `src-tauri/src/core/digest.rs`, change the signature on line 158 from:

```rust
fn extract_json(raw: &str) -> &str {
```

to:

```rust
pub(crate) fn extract_json(raw: &str) -> &str {
```

Leave `first_json_object` and everything else unchanged.

- [ ] **Step 2: Verify the whole suite still passes**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test`
Expected: all existing tests pass (same count as before this task; nothing references the new visibility yet).

- [ ] **Step 3: Verify clippy is clean**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets`
Expected: zero warnings. (`pub(crate)` on a still-internally-used function does not trigger dead-code warnings.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/core/digest.rs
git commit -m "$(cat <<'EOF'
refactor: expose extract_json as pub(crate) for reuse

The ask groundedness flow needs the same fenced-JSON extraction the
digest flow uses. No behavior change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `filter_citations` — deterministic citation fix

Replace the "all retrieved paths" behavior with "only the retrieved paths the answer actually cited, in hits-rank order, de-duplicated." This is a standalone pure function; the loop wires it in at Task 5.

**Files:**
- Modify: `src-tauri/src/core/ask.rs`

- [ ] **Step 1: Add the test-module import and write the failing tests**

At the top of the `#[cfg(test)] mod tests` block in `ask.rs`, the existing import is `use crate::core::provider::fake::FakeProvider;`. Add a `Chunk` constructor helper and the new tests. First, add this helper inside the `tests` module (used by several tests here and later):

```rust
    fn chunk(path: &str, text: &str) -> Chunk {
        Chunk {
            path: path.into(),
            chunk_id: 0,
            text: text.into(),
            vector: vec![],
        }
    }
```

Then add the three `filter_citations` unit tests inside the `tests` module:

```rust
    #[test]
    fn filter_citations_keeps_only_cited_hits_in_rank_order() {
        let a = chunk("concepts/a.md", "");
        let b = chunk("concepts/b.md", "");
        let hits = vec![&a, &b];
        // Answer cites b before a, but output is ordered by hits rank (a, then b).
        let cites = filter_citations("see [concepts/b.md] and [concepts/a.md]", &hits);
        assert_eq!(
            cites,
            vec!["concepts/a.md".to_string(), "concepts/b.md".to_string()]
        );
    }

    #[test]
    fn filter_citations_drops_uncited_and_hallucinated() {
        let a = chunk("concepts/a.md", "");
        let hits = vec![&a];
        // Cites a path that is not in hits, and does not cite a.
        let cites = filter_citations("per [concepts/ghost.md]", &hits);
        assert!(cites.is_empty());
    }

    #[test]
    fn filter_citations_dedupes_repeated_path() {
        let a0 = chunk("concepts/a.md", "");
        let mut a1 = chunk("concepts/a.md", "");
        a1.chunk_id = 1;
        let hits = vec![&a0, &a1];
        let cites = filter_citations("[concepts/a.md]", &hits);
        assert_eq!(cites, vec!["concepts/a.md".to_string()]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test filter_citations`
Expected: FAIL — `cannot find function 'filter_citations' in this scope`.

- [ ] **Step 3: Implement `filter_citations`**

Add this function in `ask.rs` (above the `ask` function, after the `use` lines):

```rust
/// The retrieved paths the answer actually cited, in hits-rank order, de-duplicated.
/// A `[path]` the answer cites that is not among `hits` is dropped (hallucinated).
fn filter_citations(answer: &str, hits: &[&Chunk]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for h in hits {
        if out.contains(&h.path) {
            continue;
        }
        if answer.contains(&format!("[{}]", h.path)) {
            out.push(h.path.clone());
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test filter_citations`
Expected: PASS (3 tests). The existing `answers_from_hits_and_dedupes_citations` test still passes here (it is rewritten in Task 5).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/ask.rs
git commit -m "$(cat <<'EOF'
feat: add filter_citations for grounded ask citations

Return only the retrieved paths the answer actually cited, in hits-rank
order, deduped — replacing the over-claiming "all hits" behavior.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: `Verdict` + `parse_verdict` — the evaluator output

The judge returns JSON `{ "verdict": "accept"|"revise"|"abstain", "feedback": "…" }`. This task adds the internal verdict type and its parser (reusing `extract_json` from Task 1). Wiring happens at Task 5.

**Files:**
- Modify: `src-tauri/src/core/ask.rs`

- [ ] **Step 1: Write the failing tests**

Add these unit tests inside the `#[cfg(test)] mod tests` block in `ask.rs`:

```rust
    #[test]
    fn parse_verdict_accept() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"accept"}"#),
            Ok(Verdict::Accept)
        ));
    }

    #[test]
    fn parse_verdict_revise_carries_feedback() {
        match parse_verdict(r#"{"verdict":"revise","feedback":"claim X unsupported"}"#) {
            Ok(Verdict::Revise(f)) => assert_eq!(f, "claim X unsupported"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[test]
    fn parse_verdict_abstain() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"abstain"}"#),
            Ok(Verdict::Abstain)
        ));
    }

    #[test]
    fn parse_verdict_handles_fenced_json() {
        assert!(matches!(
            parse_verdict("```json\n{\"verdict\":\"accept\"}\n```"),
            Ok(Verdict::Accept)
        ));
    }

    #[test]
    fn parse_verdict_unparseable_errs() {
        assert!(parse_verdict("not json").is_err());
    }

    #[test]
    fn parse_verdict_unknown_verdict_errs() {
        assert!(parse_verdict(r#"{"verdict":"maybe"}"#).is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test parse_verdict`
Expected: FAIL — `cannot find type 'Verdict'` / `cannot find function 'parse_verdict'`.

- [ ] **Step 3: Implement the verdict type and parser**

Add `use serde::Deserialize;` to the imports at the top of `ask.rs` (next to the other `use` lines). Then add this code in `ask.rs` (above `filter_citations`):

```rust
/// The judge's decision about a draft answer.
#[derive(Debug)]
enum Verdict {
    /// The draft is grounded in the context; use it.
    Accept,
    /// The draft has unsupported claims; the string is feedback for the redraft.
    Revise(String),
    /// The context does not support an answer; respond that we don't know.
    Abstain,
}

/// Why a raw judge reply could not be turned into a `Verdict`.
#[derive(Debug)]
struct VerdictError(String);

/// Raw JSON shape the judge must return.
#[derive(Deserialize)]
struct VerdictJson {
    verdict: String,
    #[serde(default)]
    feedback: String,
}

/// Parse a raw judge reply into a `Verdict`, reusing `extract_json` to peel
/// ```fences```/prose. An unparseable reply or unknown verdict word is an error;
/// the caller treats that as fail-closed (abstain).
fn parse_verdict(raw: &str) -> std::result::Result<Verdict, VerdictError> {
    let vj: VerdictJson =
        serde_json::from_str(extract_json(raw)).map_err(|e| VerdictError(e.to_string()))?;
    match vj.verdict.trim().to_ascii_lowercase().as_str() {
        "accept" => Ok(Verdict::Accept),
        "revise" => Ok(Verdict::Revise(vj.feedback)),
        "abstain" => Ok(Verdict::Abstain),
        other => Err(VerdictError(format!("unknown verdict {other:?}"))),
    }
}
```

Add `use crate::core::digest::extract_json;` to the imports at the top of `ask.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test parse_verdict`
Expected: PASS (6 tests).

- [ ] **Step 5: Verify clippy is clean**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets`
Expected: zero warnings. (`Verdict`, `VerdictError`, `VerdictJson`, and `parse_verdict` are used by tests in this task, so no dead-code warnings.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/ask.rs
git commit -m "$(cat <<'EOF'
feat: add Verdict and parse_verdict for the ask judge

Internal accept/revise/abstain verdict parsed from the judge's JSON,
reusing extract_json to tolerate fenced/prose-wrapped replies.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Prompt builders + abstention

The draft prompt (hardened), the repair prompt (carries feedback + previous draft), the judge prompts, and the canonical abstention answer. All pure/small; wired together at Task 5.

**Files:**
- Modify: `src-tauri/src/core/ask.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests inside the `#[cfg(test)] mod tests` block in `ask.rs`:

```rust
    #[test]
    fn repair_prompt_includes_feedback_and_prev_draft() {
        let p = repair_ask_prompt("q", "CTXDATA", "OLD ANSWER", "claim X unsupported");
        assert!(p.contains("claim X unsupported"));
        assert!(p.contains("OLD ANSWER"));
        assert!(p.contains("CTXDATA"));
    }

    #[test]
    fn judge_prompt_includes_draft_and_context() {
        let p = judge_user_prompt("q", "CTXDATA", "DRAFTDATA");
        assert!(p.contains("CTXDATA"));
        assert!(p.contains("DRAFTDATA"));
    }

    #[test]
    fn abstention_has_empty_citations() {
        let a = abstention();
        assert!(a.citations.is_empty());
        assert!(a.text.to_lowercase().contains("couldn't find"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib ask`
Expected: FAIL — `cannot find function 'repair_ask_prompt'` / `'judge_user_prompt'` / `'abstention'`.

- [ ] **Step 3: Implement the prompts and abstention**

Add this code in `ask.rs` (above `filter_citations`):

```rust
const ANSWER_SYSTEM: &str = "Answer the question using ONLY the provided wiki context. \
Cite the page paths you used in [brackets], e.g. [concepts/foo.md]. \
If the context does not contain the answer, say you don't know.";

const JUDGE_SYSTEM: &str = "You are a strict grounding judge. Given a QUESTION, the \
WIKI CONTEXT, and a DRAFT ANSWER, decide whether the draft is fully supported by the \
context. Respond ONLY with JSON {\"verdict\":\"accept\"|\"revise\"|\"abstain\",\"feedback\":\"...\"}. \
Use \"accept\" if every claim is supported by the context and any [bracketed] citations \
refer to provided paths. Use \"abstain\" if the context does not contain the answer. \
Use \"revise\" if the draft makes claims not supported by the context; put what is wrong \
in \"feedback\".";

/// The first-attempt draft user message.
fn base_ask_prompt(question: &str, context: &str) -> String {
    format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}")
}

/// The retry draft message: name the grounding problem, echo the previous draft,
/// restate the grounding rule, then re-append the question and context.
fn repair_ask_prompt(question: &str, context: &str, prev_draft: &str, feedback: &str) -> String {
    format!(
        "Your previous answer was not adequately grounded: {feedback}.\n\n\
         Previous answer:\n{prev_draft}\n\n\
         Revise it. Use ONLY the wiki context below and cite page paths in [brackets]. \
         Remove or correct any claim not supported by the context; if the context does \
         not support an answer, say you don't know.\n\n\
         QUESTION: {question}\n\nWIKI CONTEXT:\n{context}"
    )
}

/// The judge user message: question + context + the draft to evaluate.
fn judge_user_prompt(question: &str, context: &str, draft: &str) -> String {
    format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}\n\nDRAFT ANSWER:\n{draft}")
}

/// The canonical "couldn't ground it" answer, with no citations.
fn abstention() -> Answer {
    Answer {
        text: "I couldn't find a grounded answer for this in your wiki.".to_string(),
        citations: Vec::new(),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib ask`
Expected: the three new tests PASS. (`ANSWER_SYSTEM`, `JUDGE_SYSTEM`, and `base_ask_prompt` are not yet referenced outside this task and will warn as dead code — that is expected transient state, resolved in Task 5. Do **not** add `#[allow(dead_code)]`.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/ask.rs
git commit -m "$(cat <<'EOF'
feat: add draft/judge/repair prompts and abstention for grounded ask

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Wire the evaluator-optimizer loop into `ask`

Add `run_ask_attempts`, rewrite `ask` to delegate to it, update the existing test for the new two-call flow, and add the behavior tests. After this task all dead-code warnings from Task 4 are resolved.

> **Note on the judge:** the spec's `judge(...) -> Result<Verdict>` is realized here as an inline `provider.complete(JUDGE_SYSTEM, …)` call followed by a separate `parse_verdict(…)` inside the loop — the faithful mirror of `digest.rs`, which splits `complete()` (transport, propagated by `?`) from `evaluate()` (parse). Keeping the call and the parse separate is what lets the loop distinguish a transport error (propagate) from an unparseable verdict (fail-closed → abstain), exactly as the spec's error-handling table requires.

**Files:**
- Modify: `src-tauri/src/core/ask.rs`

- [ ] **Step 1: Replace the test-module import and rewrite the existing test**

In the `#[cfg(test)] mod tests` block of `ask.rs`, replace the import line:

```rust
    use crate::core::provider::fake::FakeProvider;
```

with:

```rust
    use crate::core::provider::fake::ScriptedProvider;
```

Then replace the existing `answers_from_hits_and_dedupes_citations` test with this updated version (the flow now makes a draft call **and** a judge call, so a single fixed-reply provider would feed the draft text to the judge and fail to parse — use `ScriptedProvider` to queue a draft then an `accept` verdict):

```rust
    #[tokio::test]
    async fn answers_from_hits_and_filters_citations() {
        let c0 = chunk("concepts/vd.md", "Vitamin D helps sleep.");
        let mut c1 = chunk("concepts/vd.md", "Take it in the morning.");
        c1.chunk_id = 1;
        let hits: Vec<&Chunk> = vec![&c0, &c1];
        let p = ScriptedProvider::new(vec![
            "Morning dose [concepts/vd.md]".into(),
            r#"{"verdict":"accept"}"#.into(),
        ]);
        let a = ask(&p, "when to take vitamin d", &hits).await.unwrap();
        assert!(a.text.contains("Morning"));
        assert_eq!(a.citations, vec!["concepts/vd.md".to_string()]);
        assert_eq!(p.calls(), 2);
    }
```

- [ ] **Step 2: Write the failing behavior tests**

Add these tests inside the `tests` module:

```rust
    #[tokio::test]
    async fn revise_then_accept() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "ungrounded [concepts/a.md]".into(),
            r#"{"verdict":"revise","feedback":"claim unsupported"}"#.into(),
            "fixed answer [concepts/a.md]".into(),
            r#"{"verdict":"accept"}"#.into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("fixed answer"));
        assert_eq!(a.citations, vec!["concepts/a.md".to_string()]);
        assert_eq!(p.calls(), 4);
    }

    #[tokio::test]
    async fn judge_abstains_returns_canonical() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "guess".into(),
            r#"{"verdict":"abstain","feedback":"not in context"}"#.into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("couldn't find a grounded answer"));
        assert!(a.citations.is_empty());
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn exhaustion_abstains() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "d1".into(),
            r#"{"verdict":"revise","feedback":"bad"}"#.into(),
            "d2".into(),
            r#"{"verdict":"revise","feedback":"still bad"}"#.into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("couldn't find a grounded answer"));
        assert!(a.citations.is_empty());
        assert_eq!(p.calls(), 4);
    }

    #[tokio::test]
    async fn unparseable_judge_abstains() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "answer [concepts/a.md]".into(),
            "not json".into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("couldn't find a grounded answer"));
        assert!(a.citations.is_empty());
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn transport_error_propagates_without_retry() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        // Empty queue makes the first complete() call error, standing in for a
        // transport failure: it must propagate, not abstain.
        let p = ScriptedProvider::new(vec![]);
        let err = ask(&p, "q", &hits).await.unwrap_err();
        assert!(format!("{err}").contains("exhausted"));
        assert_eq!(p.calls(), 1);
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib ask`
Expected: FAIL — the new behavior tests fail because `ask` does not yet make a judge call (`run_ask_attempts` does not exist; `ask` still returns all hits as citations and never calls a judge, so `calls()` and abstention assertions fail).

- [ ] **Step 4: Implement `run_ask_attempts` and rewrite `ask`**

Replace the **entire current body** of the `ask` function with a delegation, and add the loop above it. The final non-test region of `ask.rs` should read:

```rust
/// Ask the LLM to answer `question` grounded ONLY in the pre-retrieved `hits`,
/// verifying each draft with an LLM judge and retrying with feedback. Returns a
/// grounded answer (citing only retrieved paths it used) or a canonical
/// abstention when the wiki cannot ground an answer. Transport errors propagate.
pub async fn ask(provider: &dyn LlmProvider, question: &str, hits: &[&Chunk]) -> Result<Answer> {
    const MAX_ATTEMPTS: usize = 2;
    run_ask_attempts(provider, question, hits, MAX_ATTEMPTS).await
}

/// Draft → judge → repair loop. Each iteration makes one draft call and one judge
/// call (both propagate transport errors via `?`). `accept` returns the draft with
/// filtered citations; `abstain`, an unparseable verdict, or attempt exhaustion
/// returns the canonical abstention; `revise` re-drafts with feedback.
async fn run_ask_attempts(
    provider: &dyn LlmProvider,
    question: &str,
    hits: &[&Chunk],
    max_attempts: usize,
) -> Result<Answer> {
    let context = hits
        .iter()
        .map(|h| format!("[{}]\n{}", h.path, h.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut last: Option<(String, String)> = None; // (prev_draft, feedback)
    for _ in 1..=max_attempts {
        let user = match &last {
            None => base_ask_prompt(question, &context),
            Some((prev_draft, feedback)) => {
                repair_ask_prompt(question, &context, prev_draft, feedback)
            }
        };
        let draft = provider.complete(ANSWER_SYSTEM, &user).await?;

        let verdict_user = judge_user_prompt(question, &context, &draft);
        let verdict_raw = provider.complete(JUDGE_SYSTEM, &verdict_user).await?;

        match parse_verdict(&verdict_raw) {
            Ok(Verdict::Accept) => {
                let citations = filter_citations(&draft, hits);
                return Ok(Answer {
                    text: draft,
                    citations,
                });
            }
            Ok(Verdict::Abstain) => return Ok(abstention()),
            Ok(Verdict::Revise(feedback)) => last = Some((draft, feedback)),
            // Fail-closed: we cannot certify groundedness, so abstain.
            Err(_) => return Ok(abstention()),
        }
    }
    Ok(abstention())
}
```

The old citation-building block (the `for h in hits { … citations.push … }` loop and the single `provider.complete(system, &user)` call) is fully removed — `filter_citations` and the loop replace it.

- [ ] **Step 5: Run the full test suite**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test`
Expected: PASS. All ask tests (units from Tasks 2–4 plus the six behavior tests) and every pre-existing test pass.

- [ ] **Step 6: Verify clippy and formatting**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets`
Expected: zero warnings (the Task 4 dead-code warnings are now resolved — every prompt/const is used by `run_ask_attempts`).

Run: `cd src-tauri && $HOME/.cargo/bin/cargo fmt`
Then confirm only `ask.rs` changed: `git diff --name-only` shows `src-tauri/src/core/ask.rs`.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/core/ask.rs
git commit -m "$(cat <<'EOF'
feat: self-correcting grounded ask with judge + repair loop

Wrap the ask RAG call in a bounded evaluator-optimizer loop: draft, then
an LLM judge returning accept/revise/abstain. Accept returns the draft
with citations filtered to the retrieved paths it used; revise re-drafts
with feedback; abstain, an unparseable verdict, or attempt exhaustion
returns a canonical abstention. Transport errors propagate. The Answer
shape is unchanged, so commands and the frontend are untouched.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Final verification (after all tasks)

- [ ] `cd src-tauri && $HOME/.cargo/bin/cargo test` — all green.
- [ ] `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets` — zero warnings.
- [ ] `cd src-tauri && $HOME/.cargo/bin/cargo fmt` — clean.
- [ ] `git diff --name-only main..HEAD` — only `src-tauri/src/core/digest.rs`, `src-tauri/src/core/ask.rs`, and the two `docs/superpowers/` files.
- [ ] Confirm `commands.rs`, `state.rs`, the `LlmProvider` trait, and the frontend are untouched (the `Answer { text, citations }` shape is unchanged).
```
```
