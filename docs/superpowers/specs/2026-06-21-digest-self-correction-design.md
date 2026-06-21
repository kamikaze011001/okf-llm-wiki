# Digest Self-Correction — Design

**Status:** Approved (design); pending implementation plan
**Date:** 2026-06-21
**Author:** brainstorming session (okf-llm-wiki)

## Problem

The `digest` flow (`src-tauri/src/core/digest.rs`) makes a single one-shot LLM
call and parses the reply as JSON. If the model returns malformed JSON — or valid
JSON with a blank `title` or `body` — `digest` fails immediately and the whole
capture is lost. There is no second chance, even though LLMs reliably fix their own
output when told exactly what was wrong.

This is the brittleness this feature removes.

## Goal

Make `digest` resilient to malformed model output by adding a **bounded
evaluator-optimizer retry loop**: generate → evaluate → on failure, feed the
failure back and retry, capped at a small number of attempts. Reliability win,
minimal new surface area.

This is the lightest form of the evaluator-optimizer pattern from Anthropic's
"Building Effective Agents." We deliberately stop short of heavier patterns
(orchestrator-workers, autonomous agents) — the app does not need them.

## Scope

**In scope:** the digest flow only.

**Out of scope (YAGNI):**
- Configurable attempt count (hard-coded constant is fine).
- Retrying transport/network errors (a dead network won't fix itself).
- The `ask` (RAG) groundedness check — that is a separate follow-up spec.
- Structural enforcement of body format (TL;DR line, `## Key points`).
- Any new logging, telemetry, or UI surface (attempt count is not surfaced).
- Changing the `LlmProvider` trait.

## Design

### Architecture

A single bounded loop inside `digest()`. **No changes** to the `LlmProvider`
trait, `commands.rs`, `state.rs`, the OKF store, or the frontend. `digest()` keeps
its exact current signature — only its internals change. The caller still receives
either one `DigestResult` or one error.

The system prompt (which carries the JSON contract and the `[[wikilink]]`
allow-list) stays **identical across all attempts**. Only the *user* message
changes on a retry.

### Components

**1. `evaluate(raw: &str) -> Result<DigestJson, DigestFailure>`** — the evaluator.

Runs the existing `extract_json` → `serde_json::from_str::<DigestJson>` chain, then
checks that `title` and `body` are non-empty after `trim()`. Returns the parsed
struct on success, or a `DigestFailure` describing what went wrong. `extract_json`
and `first_json_object` are reused unchanged.

`DigestFailure` is an internal enum with a human-readable `Display` used both to
build the repair prompt and to form the final error message. Variants:
- `Unparseable(String)` — the `serde_json` error text.
- `EmptyField(&'static str)` — `"title"` or `"body"`.

**2. `repair_user_prompt(base_user, prev_raw, failure) -> String`** — the optimizer
input. Rebuilds the user message with a correction preamble that states the exact
failure, includes the previous raw reply, and restates the requirement. Shape:

```
Your previous reply could not be used: <failure>.

Previous reply:
<prev_raw>

Respond ONLY with valid JSON {"title":..,"description":..,"tags":[..],"body":..}.
Both "title" and "body" must be non-empty.

<base_user>
```

`base_user` is the original `SOURCE:\n…\n\nUSER NOTE:` message, re-appended so the
model still has the source material to work from.

**3. The loop in `digest()`** — up to **3 attempts** (`const MAX_ATTEMPTS: usize = 3;`
— one initial call plus two error-feedback retries).

- Attempt 1 calls `complete(system, base_user)`.
- Attempts 2–3 call `complete(system, repair_user_prompt(base_user, prev_raw, failure))`,
  built from the immediately preceding attempt's raw output and failure.
- The first successful `evaluate` breaks out and proceeds to link validation.
- After the final attempt fails, return `Err`.

### Data flow

```
submit_source → fetch_clean → digest():
   attempt 1: complete(system, base_user)        → evaluate → Ok? → done
   attempt 2: complete(system, repair(prev_raw₁)) → evaluate → Ok? → done
   attempt 3: complete(system, repair(prev_raw₂)) → evaluate → Ok? → done : Err
→ slugify → validate_links (unchanged) → build Page → DigestResult
```

Externally identical to today: one `DigestResult` or one error.

### Error handling

- **Transport error** from `complete()` (network/HTTP) → propagates **immediately,
  no retry**. The retry loop is only for output-quality failures (parse / empty
  field).
- **Evaluate failure** on attempts 1–2 → retry with feedback.
- **Evaluate failure** on the final attempt → return `Err` with a message that
  preserves today's behavior of surfacing the problem, e.g.:
  `"digest failed after 3 attempts: <last failure>"`.
- `validate_links` link sanitization runs once, after a successful parse —
  unchanged.

## Testing

All tests run offline against test doubles — no live network.

### New test double (additive)

`FakeProvider` currently returns a single fixed reply and is used as a struct
literal (`FakeProvider { reply: … }`) across ~6 existing tests. To avoid churning
those, **add a separate scripted double** rather than changing `FakeProvider`:

```rust
// src-tauri/src/core/provider/fake.rs
pub struct ScriptedProvider {
    replies: std::sync::Mutex<std::collections::VecDeque<String>>,
    calls: std::sync::atomic::AtomicUsize,
}
```

- `ScriptedProvider::new(replies: Vec<String>)` — queue of replies, returned in order.
- `complete()` pops the front reply, increments `calls`; if the queue is empty it
  returns an `Err` (an unexpected extra call fails loudly rather than silently
  repeating).
- `calls() -> usize` — lets tests assert the exact number of attempts made.

`FakeProvider` is left exactly as-is, so all existing digest tests keep passing.

### Test cases

1. **Bad-then-good (parse):** `["not json", <valid JSON>]` → `digest` succeeds with
   the second page; assert `calls() == 2`.
2. **Bad-then-good (empty field):** `[<valid JSON, empty title>, <valid JSON>]` →
   succeeds; assert `calls() == 2`.
3. **Exhaustion:** three malformed replies → `digest` returns `Err` after exactly
   3 attempts; assert `calls() == 3` and the error mentions the failure.
4. **Happy path unchanged:** `[<valid JSON>]` → succeeds on attempt 1; assert
   `calls() == 1` (no retry).
5. **`evaluate()` unit tests:** unparseable input → `Unparseable`; empty `title` →
   `EmptyField("title")`; empty `body` → `EmptyField("body")`; valid → `Ok`.

All existing `digest.rs` tests must continue to pass unchanged.

## Security

`digest.rs` is not a listed sensitive area, but this change touches
`src-tauri/src/core/provider/fake.rs`, which lives under `provider/` (a gated
area). `fake.rs` / `ScriptedProvider` is a test-only double with no key handling
and no outbound requests, so the risk is nil — but to honor the workflow gate,
run the `security-reviewer` agent before committing.

No API key, source text, or model output is logged; error messages already in
`digest.rs` include the raw reply, and this design does not change that surface.
