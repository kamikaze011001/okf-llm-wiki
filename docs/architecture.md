# Architecture — okf-llm-wiki

> Status: v1 (thin vertical slice). Last updated 2026-06-16.

## Overview

okf-llm-wiki is a **local-first desktop application** that turns saved links and notes into a personal, queryable knowledge base. It is a **modular monolith**: a single Tauri 2 process with a Rust core, an IPC command bridge, and a SvelteKit SPA frontend running in the system webview. All state lives on the user's machine — there is no server, and the only network egress is to the configured LLM provider.

## The core loop

```
paste URL / text
      │
      ▼
 fetch + clean        (core/fetch.rs: classify Source, html_to_text)
      │
      ▼
 LLM digest           (core/digest.rs → LlmProvider.complete → DigestJson)
      │
      ▼
 write OKF page       (core/store.rs: atomic write concepts/<slug>.md + append log.md)
      │
      ▼
 build retrieval index (core/retrieval.rs: hash_embed + cosine over all pages)
      │
      ├─► Browse       (list_pages → PageDto → Browse.svelte)
      └─► Ask          (core/ask.rs: search top-k → LlmProvider.complete → Answer + citations)
```

## Layers

### Core domain — `src-tauri/src/core/`

Pure, framework-agnostic logic. No Tauri types leak in here.

- `slug.rs` — title → filesystem slug
- `page.rs` — `Page` + `Frontmatter` (serde) — the in-memory OKF page model
- `store.rs` — `OkfStore`: atomic write (temp file + rename), manual `---` frontmatter-fence parse + serde_yaml, `list_pages` (recursive, excludes `index.md`/`log.md`), `append_log`. Round-trip safe for externally-authored files (EOF-fence + extra-key preservation).
- `provider/` — `LlmProvider` async trait (`complete`, `embed`); `fake.rs` (deterministic test double); `claude.rs` (Anthropic messages API). **Provider-agnostic** — OpenAI/Ollama are future adapters.
- `retrieval.rs` — `hash_embed` (deterministic local FNV-style hashing embedder, DIM=256), `cosine`, `IndexEntry`, `search`, `build_index`. Keeps search **offline** and provider-independent in v1.
- `fetch.rs` — `Source` classification, URL → cleaned text (`scraper`), text passthrough
- `digest.rs` — `digest()`: source text → `DigestJson` (LLM contract) → `Page` + log entry
- `ask.rs` — `ask()`: retrieve top-k context → prompt provider → `Answer { text, citations }`
- `settings.rs` — `Settings { provider, model, api_key, wiki_path }`, `make_provider()` factory

### IPC bridge — `src-tauri/src/commands.rs`

Thin Tauri `#[command]` wrappers exposing the core to the frontend. DTOs (`PageDto`, `AnswerDto`) and `Settings` are the wire contract shared with `src/lib/api.ts`.

- Sync: `get_settings`, `set_settings`, `list_pages`
- Async: `submit_source { input, note }`, `ask_question { question }`

**Concurrency rule:** these handlers read shared state via `Mutex`. A `MutexGuard` must never be held across an `.await` (not `Send`). Pattern: `let s = state.settings.lock().unwrap().clone();` then await.

### App shell (Rust) — `src-tauri/src/`

- `state.rs` — `AppState { settings: Mutex<Settings>, index: Mutex<Vec<IndexEntry>> }`
- `lib.rs` — Tauri builder: plugins (`opener`, `notification`), `manage(AppState)`, `invoke_handler`, `run()`

### Frontend — `src/lib/` + `src/routes/`

- `src/lib/api.ts` — typed wrappers over `@tauri-apps/api` `invoke`; mirrors the Rust DTOs exactly
- `src/lib/stores.ts` — `route` + `currentPage` writable stores (store-driven SPA navigation)
- `src/lib/components/` — `Rail`, `Home`, `Browse`, `Ask`, `Settings` (neo-brutalist)
- `src/lib/styles/neobrutal.css` — design tokens (3px ink borders, hard offset shadows, flat accents)
- `src/routes/+layout.svelte` — imports global CSS, renders the page (runes)
- `src/routes/+page.svelte` — the app shell: rail + `{#if $route===…}` panel switch

The frontend is **SPA mode** (`adapter-static`, `+layout.ts` `ssr=false`). There is no server; every data dependency is a Tauri command.

## Data: Open Knowledge Format (OKF)

A page is `concepts/<slug>.md` = YAML frontmatter (`type`, `title`, `description`, `tags`, `resource`, `timestamp`, `note`, plus preserved extra keys) + Markdown body (TL;DR-first). `log.md` is an append-only activity log. Files are **portable**: readable and editable without the app.

## External dependencies

| Dependency | Purpose | Config |
|---|---|---|
| Claude API | LLM for digest + ask | `Settings.api_key` / `model` (in-memory) |
| Local filesystem | OKF wiki folder | `Settings.wiki_path` |

No database, queue, or cache.

## Known v1 limitations / follow-ups

1. Settings (incl. API key) are **in-memory only** — re-entered each launch. Persistence is the top v1.1 task.
2. `digest.rs` JSON parse assumes bare JSON — harden against ```-fenced / prose-wrapped model output.
3. `now_iso()` emits `unixtime:NNN` rather than ISO-8601 (cosmetic).
4. `Browse.svelte` `$: pick()` is an ineffective reactive statement (works only because the component remounts per route).
