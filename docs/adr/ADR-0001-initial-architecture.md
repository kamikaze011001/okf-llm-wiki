# ADR-0001: Initial architecture for okf-llm-wiki v1

- **Status:** Accepted
- **Date:** 2026-06-16
- **Deciders:** project owner

## Context

We are building a **local-first, personal LLM Wiki**: paste a URL or note, have an LLM digest it into a portable knowledge page, then browse and ask questions over the accumulated knowledge. Priorities: zero-friction capture, the user owns their data on disk, the LLM provider is swappable, and the UI deliberately avoids generic "AI slop" (neo-brutalist design).

## Decisions

### 1. Tauri 2 desktop app (Rust core + system webview), not a web service

Local-first and private by default: data and compute stay on the user's machine, the only egress is to the chosen LLM provider. Rust gives a safe, fast core; Tauri ships a small binary using the OS webview. **Consequence:** no server to operate; all frontend↔backend communication is Tauri IPC commands.

### 2. SvelteKit + Svelte 5 in SPA mode (`adapter-static`, `ssr=false`)

A static SPA loaded in the webview. No SSR, no server routes — every data dependency flows through a Tauri command. **Consequence:** `src/routes/` is a thin store-driven shell; logic lives in `src/lib/` and the Rust core.

### 3. Open Knowledge Format (OKF) on disk as the source of truth

Each page is `concepts/<slug>.md` = YAML frontmatter + Markdown body; `log.md` is an append-only activity log. Writes are atomic (temp file + rename). **Consequence:** the knowledge base is portable, diff-able, and usable without the app; we maintain a hand-rolled frontmatter parser that tolerates externally-authored files.

### 4. Provider-agnostic `LlmProvider` trait

A single async trait (`complete`, `embed`) with a `make_provider` factory. Claude is the only v1 adapter; OpenAI/Ollama are future adapters. **Consequence:** no provider specifics leak outside `provider/` and the factory.

### 5. Local deterministic hashing embedder for retrieval (no provider embeddings in v1)

`hash_embed` (FNV-style, DIM=256) + cosine similarity over all pages. **Consequence:** search is fully offline, deterministic, and provider-independent; quality is coarse. A real embeddings adapter is a planned upgrade.

### 6. Modular monolith, core kept framework-agnostic

`src-tauri/src/core/` holds pure domain logic with no Tauri types; `commands.rs` is the only Tauri-aware bridge. **Consequence:** the core is unit-testable in isolation; the concurrency rule "never hold a `MutexGuard` across `.await`" applies in `commands.rs`.

## Consequences / known limitations (v1)

- Settings (including the API key) are **in-memory only** — not persisted across restarts. Persistence is the top v1.1 task.
- `digest.rs` expects bare JSON from the model; needs hardening against fenced/prose-wrapped output.
- Retrieval quality is limited by the hashing embedder.

## Alternatives considered

- **Electron** — rejected: larger binary, heavier runtime, no Rust core.
- **Web app + hosted backend** — rejected: violates local-first/privacy goal, adds ops burden.
- **SQLite / proprietary store** — rejected: OKF Markdown keeps data portable and human-editable.
- **Provider embeddings for search** — deferred: adds network dependency and provider lock-in for v1.
