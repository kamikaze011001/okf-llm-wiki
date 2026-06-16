# okf-llm-wiki — Roadmap to a Complete App

> Milestones to take okf-llm-wiki from a working v1 thin slice to a complete, zero-barrier personal LLM Wiki.
> Each milestone is independently shippable and produces a usable improvement on its own.

**Last updated:** 2026-06-16 · **Current release:** v1 (thin vertical slice, on `main`)

---

## Where we are today (v1)

The full capture→digest→browse→ask loop works end-to-end:

```
paste URL/text → fetch+clean → LLM digest → OKF page (concepts/<slug>.md + log.md)
              → hashing-embedder index → Browse + Ask (with citations)
```

**Built & green:** 17 Rust tests + 2 frontend tests, `cargo clippy` clean, both builds exit 0.
Stack: Tauri 2 (Rust core) + SvelteKit/Svelte 5 SPA, neo-brutalist UI.

### Honest gaps in v1 (found by reading the code)

| Gap | Where | Impact |
|---|---|---|
| Settings are **in-memory only** | `state.rs` (`#[derive(Default)]`), `commands.rs:16-19` | API key + wiki folder reset on **every restart** |
| Retrieval index **never built on startup** | `commands.rs:43` (only rebuilt in `submit_source`) | After a restart, **Ask returns nothing** until you add a new page |
| Digest JSON parse is **brittle** | `digest.rs:25` (`serde_json::from_str(raw.trim())`) | Real Claude output wrapped in ```` ```json ```` fences or prose → **digest fails** |
| Timestamps are **not real ISO-8601** | `digest.rs:45-50` (`unixtime:NNN`) | OKF frontmatter is less portable / sortable |
| Retrieval quality is **coarse** | `retrieval.rs` (FNV hashing embedder, DIM=256) | Ask finds loosely-related pages, not semantically best ones |
| No way to **edit or delete** a page in-app | (not implemented) | Mistakes require hand-editing files on disk |
| Pages are **not interlinked** | (not implemented) | It's a flat list of notes, not a *wiki* |

These gaps map directly onto the milestones below.

---

## Milestones

Ordered by dependency and value. M1 is the foundation; M2–M4 can be reordered after it.

### M1 — Make v1 trustworthy *(foundation — do first)*

**Delivers:** an app you can actually use day-to-day without it forgetting your setup or failing on real input.

- Persist Settings across restarts — API key in the **OS keychain**, the rest in an app-config file.
- Build the retrieval index **on startup** (and when the wiki folder changes), so Ask works immediately after launch.
- Harden digest JSON parsing — tolerate ```` ```json ```` fences and prose-wrapped output.
- Emit real ISO-8601 timestamps.
- *(stretch)* Clean up the ineffective `Browse.svelte` reactive statement.

**Why first:** every other milestone assumes settings persist and the app is reliable. This is the smallest, highest-value step.
**Touches sensitive areas** (`settings.rs`, `state.rs`) → security review before commit.

### M2 — Make Ask genuinely good *(retrieval quality)*

**Delivers:** answers grounded in the *semantically* most relevant pages, not just hash-collision-adjacent ones.

- Replace the hashing embedder with real embeddings behind the existing `LlmProvider::embed` boundary (provider embeddings, or a local embedding model for offline use).
- Persist the index to disk so it isn't rebuilt from scratch each launch as the wiki grows.
- Add basic chunking for long pages so retrieval is page-section accurate.

**Why:** Ask quality is the core value of the product; the v1 embedder was a deliberate offline placeholder.
**Depends on:** M1 (stable settings/provider). **Touches** `provider/` → security review.

### M3 — Make it a real *wiki* *(interlinked knowledge)*

**Delivers:** the Karpathy "LLM Wiki" soul — concepts that link to each other and accumulate into a graph.

- `[[concept]]` wiki-links in OKF page bodies, rendered as in-app navigation.
- **Backlinks** — each page shows what links to it.
- **Auto-linking** at digest time — the LLM (or a post-pass) links new pages to existing concepts.
- A browsable **concept graph / index** view.

**Why:** transforms a flat note list into compounding, navigable knowledge. This is what makes it a *wiki*.
**Depends on:** M1. Independent of M2.

### M4 — Zero-barrier UX *(effortless for a lazy learner)*

**Delivers:** the original north star — anyone learns new things with no friction.

- **Edit & delete** pages in-app (today you must hand-edit files).
- **Frictionless capture** — paste from clipboard, drag-drop a link/file, optional global hotkey.
- **Onboarding** — first-run wizard (set wiki folder + key), empty states, loading/progress feedback.
- Keyboard-driven navigation; quick-switcher between pages.

**Why:** removes the last barriers between "I saw something interesting" and "it's in my wiki and I understand it."
**Depends on:** M1. Best after M3 (so editing understands links).

---

## Recommended sequence

```
M1 (trustworthy)  →  M2 (smart answers)  →  M3 (real wiki)  →  M4 (zero-barrier UX)
   foundation          core value            wiki soul           polish
```

M2 and M3 are independent of each other; either can come second based on appetite. M1 is non-negotiably first.

## Cross-cutting (apply within each milestone, not separate work)

- Keep `cargo clippy --all-targets` warning-free; `cargo fmt` before commit; tests green.
- OKF files stay portable Markdown + YAML — every milestone preserves round-trippable, app-free files.
- Security review before committing changes to `settings.rs`, `state.rs`, `store.rs`, `provider/`.
- Neo-brutalist design language for all new UI.

## Out of scope (deferred beyond "complete")

Multi-device sync · multiple LLM providers wired (OpenAI/Ollama adapters) · sharing/export to other tools · mobile. Revisit once M1–M4 land.
