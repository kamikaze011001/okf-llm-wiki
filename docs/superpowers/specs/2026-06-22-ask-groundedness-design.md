# Ask Groundedness — Design

**Status:** Approved (design); pending implementation plan
**Date:** 2026-06-22
**Author:** brainstorming session (okf-llm-wiki)

## Problem

The `ask` flow (`src-tauri/src/core/ask.rs`) is a plain one-shot RAG call: it
retrieves chunks, makes a single LLM call, and returns the reply verbatim. Two
trust problems follow:

1. **Citations overclaim.** The returned `citations` are *every retrieved page
   path, de-duplicated* (`ask.rs:19-29`) — not the pages the answer actually
   used. A user sees citations the answer never drew from.
2. **No groundedness guarantee.** Nothing checks that the answer's claims are
   supported by the retrieved context, or that the model abstained when the wiki
   genuinely lacks the answer. The model can answer from its own training instead
   of the wiki, and the user has no signal.

This is the brittleness this feature removes — the `ask`-side sibling of the
digest self-correction already shipped.

## Goal

Make `ask` answers **verifiably grounded** by wrapping the RAG call in a
**bounded evaluator-optimizer loop** with an **LLM-as-judge**, plus a
deterministic citation fix. Generate → judge → on failure repair with feedback →
abstain if it still can't be grounded.

This is the same evaluator-optimizer pattern from Anthropic's "Building Effective
Agents" used for digest, now with a *semantic* (LLM) evaluator because the checks
("is this claim supported?", "should it have abstained?") cannot be done by
string matching.

## Scope

**In scope:** the `ask` flow (`src-tauri/src/core/ask.rs`) only.

**Out of scope (YAGNI):**
- Retrieval / embedding quality (that is milestone M2 — separate).
- Changing the `LlmProvider` trait.
- Changing `commands.rs`, `state.rs`, or the frontend. The `Answer { text,
  citations }` shape stays identical — abstention is a normal answer (canonical
  text + empty citations). A `grounded: bool` flag for the UI is a noted
  follow-up, not this spec.
- Configurable attempt count (hard-coded constant).
- Retrying transport / network errors (a dead network won't fix itself —
  propagate immediately).
- Streaming answers.

## Design

### Architecture

A bounded loop inside the `ask` flow, mirroring `digest.rs`. **No changes** to
the `LlmProvider` trait, `commands.rs`, `state.rs`, the OKF store, or the
frontend. The public `Answer` shape is unchanged. Four internal pieces in
`ask.rs`:

**1. draft** — generate an answer from the retrieved hits (today's call, with a
hardened system prompt that insists on grounding and `[path]` citations).

**2. `filter_citations(answer: &str, hits: &[&Chunk]) -> Vec<String>`** —
deterministic. Parse `[path]` tokens out of the answer text; return the subset of
`hits` paths that the answer cited, ordered by **hits rank** (the order paths
first appear in `hits`), de-duplicated. This replaces the current "all retrieved
paths" behavior and fixes the overclaim bug. A `[path]` the answer cites but that
is not in `hits` is dropped (a hallucinated citation). Ordering by hits rank
(rather than answer-appearance order) preserves today's rank-ordered citation
behavior.

**3. `judge(provider, question, context, draft) -> Result<Verdict>`** — a second
LLM call. Given the question, the retrieved context, and the draft answer, it
returns a structured JSON verdict:

```json
{ "verdict": "accept" | "revise" | "abstain", "feedback": "…" }
```

- `accept` — the draft is grounded in the context; use it.
- `revise` — the draft makes unsupported claims that should be removed or
  grounded; `feedback` says what is wrong.
- `abstain` — the context does not support an answer; the response should say it
  doesn't know.

Parsed via the same `extract_json` helper the digest flow uses; that helper is
promoted to `pub(crate)` in `digest.rs` so both flows share it (DRY) rather than
duplicating it.

**4. `run_ask_attempts(provider, question, hits, max_attempts) -> Result<Answer>`**
— the loop. `const MAX_ATTEMPTS: usize = 2;` (one initial draft plus one repair).

### Verdict model

Internal enum, mirroring `DigestFailure`:

```rust
enum Verdict { Accept, Revise(String), Abstain }
fn parse_verdict(raw: &str) -> std::result::Result<Verdict, VerdictError>
```

`VerdictError` carries the parse failure for the fail-closed path below. (As with
digest, `anyhow::Result` shadows `Result` to a one-type-arg alias in this file,
so `parse_verdict` spells out the two-arg `std::result::Result`.)

### Data flow

Retrieval happens upstream and is unchanged.

```
run_ask_attempts(provider, question, hits, MAX_ATTEMPTS):
  context = hits.map("[path]\n{text}").join("\n\n")     (as today)
  last_feedback: Option<(String draft, String feedback)> = None
  loop up to MAX_ATTEMPTS:
    user = base_ask_prompt(question, context)                       (attempt 1)
         | repair_ask_prompt(question, context, prev_draft, feedback) (retry)
    draft   = provider.complete(answer_system, &user).await?   ← transport err propagates
    verdict = judge(provider, question, &context, &draft).await? ← transport err propagates
    match verdict:
      Accept          → return Answer { text: draft, citations: filter_citations(&draft, hits) }
      Abstain         → return abstention()                       (terminal)
      Revise(fb)      → last_feedback = Some((draft, fb)); continue
  exhausted → return abstention()

abstention() = Answer {
  text: "I couldn't find a grounded answer for this in your wiki.",
  citations: vec![],
}
```

Externally the caller still receives one `Answer` (or one transport error), as
today.

### Error handling

Three distinct failures, handled differently:

| Failure | Source | Behavior |
|---|---|---|
| Transport error | either `complete()` call | Propagate immediately via `?` — no retry (matches digest). |
| Unparseable judge verdict | judge reply is not valid JSON per `parse_verdict` | **Fail-closed → abstain.** We cannot certify groundedness, and trustworthy beats convenient. Terminal, no extra loop. |
| `Revise` verdict | judge rejects the draft | Repair with `feedback`, consume an attempt; abstain on exhaustion. |

Note the asymmetry vs. digest: there, unparseable output came from the
*generator*, so retrying the generator was the fix. Here unparseable output comes
from the *judge* — re-drafting would not fix a flaky judge — so an unparseable
verdict fails closed to abstention rather than looping.

## Testing

All tests run offline against `ScriptedProvider` (added during the digest work;
queues replies and counts calls). Each loop attempt consumes **two** queued
replies: the draft, then the judge verdict.

### Unit tests

- `filter_citations`: drops a `[path]` not present in `hits`; de-dupes repeated
  citations; preserves rank order; an answer that cites nothing yields `[]`.
- `parse_verdict`: `accept` → `Accept`; `revise` (+feedback) → `Revise(feedback)`;
  `abstain` → `Abstain`; non-JSON / missing field → `Err(VerdictError)`.

### Behavior tests (through the loop)

1. **Accept on first attempt:** `[draft, judge=accept]` → returns the draft with
   filtered citations; `calls() == 2`.
2. **Revise then accept:** `[draft1, judge=revise, draft2, judge=accept]` →
   returns `draft2`; `calls() == 4`.
3. **Judge abstains:** `[draft, judge=abstain]` → canonical abstention, empty
   citations; `calls() == 2`.
4. **Exhaustion → abstain:** `[draft1, judge=revise, draft2, judge=revise]`
   (`MAX_ATTEMPTS == 2`) → abstention; `calls() == 4`.
5. **Unparseable judge → abstain (fail-closed):** `[draft, "not json"]` →
   abstention; `calls() == 2`.
6. **Transport error propagates:** empty queue → first `complete()` errors →
   `ask` returns `Err`, not an abstention.

### Existing test

`answers_from_hits_and_dedupes_citations` currently expects a single provider
call and no judge. Update it to the new flow (queue a draft plus an `accept`
verdict) and assert the citations are the *filtered* set, not all hits.

## Security

`ask.rs` is **not** a listed sensitive area, and this change touches **no** gated
file:

- `ScriptedProvider` already exists, so there is no `provider/` change.
- Promoting `extract_json` to `pub(crate)` in `digest.rs` is not a sensitive-area
  change.

So no mandatory security-review gate is triggered. For completeness: the judge
prompt embeds the retrieved wiki text and the draft answer (no API key, no
secrets); the abstention text and verdict feedback contain no secret material;
and nothing is logged — mirroring digest's no-logging stance. The final answer
still flows only back to the caller, exactly as today.
