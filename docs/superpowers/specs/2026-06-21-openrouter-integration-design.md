# OpenRouter Integration — Design

**Status:** Approved (2026-06-21)
**Goal:** Add OpenRouter as a second chat-completion provider alongside Claude, reachable with a single OpenRouter API key, with model selection via a fetched, filterable list. Embeddings are untouched.

## Context

The app has a one-method LLM provider trait:

```rust
// src-tauri/src/core/provider/mod.rs
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String>;
}
```

`ClaudeProvider` is the only implementation. `make_provider` (`settings.rs:60`) matches on `Settings::provider` and errors on anything but `"claude"`. The Settings UI dropdown currently *lists* `openai`/`ollama` as completion-provider options, but they are not wired and error at runtime. (`ollama` is real, but only as an **embedding** provider — it already lives in the embedding selector.)

A single API key is stored in the OS keychain under one account (`"api_key"`), loaded into `Settings.api_key` at startup. Switching provider reuses whatever key is in that slot.

OpenRouter exposes an OpenAI-compatible `POST /api/v1/chat/completions` endpoint (Bearer auth, model as a namespaced string like `openai/gpt-4o`) and a **keyless** `GET /api/v1/models` endpoint. This is a clean fit for the existing `complete()` trait.

## Decisions (from brainstorming)

- **Scope:** chat completions only — digest + ask. Embeddings stay on hash/ollama. Out of scope.
- **Key storage:** keep the existing single shared keychain slot. Switching to OpenRouter means pasting its key over the Anthropic one (and back). No per-provider keys, no keychain refactor.
- **Model selection:** fetched from OpenRouter's `/models` endpoint, presented as a **search/filter box**. The raw model string stays editable so arbitrary IDs still work.

## Architecture

### 1. Backend — provider (`src-tauri/src/core/provider/openrouter.rs`, new)

`OpenRouterProvider { api_key: String, model: String, client: reqwest::Client }` implementing `LlmProvider::complete`, structurally mirroring `ClaudeProvider`:

- `POST https://openrouter.ai/api/v1/chat/completions`
- Headers: `Authorization: Bearer <api_key>`, and a static `X-Title: okf-llm-wiki` (OpenRouter's optional attribution header — carries no user data).
- Body (OpenAI chat shape):
  ```json
  {
    "model": "<model>",
    "max_tokens": 4096,
    "messages": [
      { "role": "system", "content": "<system>" },
      { "role": "user", "content": "<user>" }
    ]
  }
  ```
- Response: parse `choices[0].message.content` as a string; missing/wrong shape → `anyhow!` error.
- Non-2xx: `anyhow!` error with status + response body, exactly like `ClaudeProvider`. **The API key is never logged or echoed.**
- A `pub(crate) fn chat_body(&self, system: &str, user: &str) -> serde_json::Value` helper (parallels Claude's `messages_body`), unit-tested.

### 2. Backend — model listing (same file)

```rust
#[derive(Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,   // e.g. "openai/gpt-4o"
    pub name: String, // human label from the API; falls back to id if absent
}
```

- `pub fn parse_models(v: &serde_json::Value) -> Vec<ModelInfo>` — pure: reads `data[]`, takes `id` (skips entries with no `id`) and `name` (defaults to `id` when absent). Unit-tested from sample JSON, no network.
- `pub async fn fetch_models(client: &reqwest::Client) -> Result<Vec<ModelInfo>>` — `GET https://openrouter.ai/api/v1/models` (no auth), non-2xx → error, then `parse_models`.

### 3. Wiring

- `make_provider` (`settings.rs:60`): add arm
  ```rust
  "openrouter" => Ok(Arc::new(OpenRouterProvider::new(s.api_key.clone(), s.model.clone()))),
  ```
  *(sensitive file — security review)*
- `provider/mod.rs`: `pub mod openrouter;`
- New IPC command in `commands.rs`:
  ```rust
  #[tauri::command]
  pub async fn list_openrouter_models() -> Result<Vec<ModelInfo>, String>
  ```
  Takes no arguments (the endpoint is keyless) and touches no shared state, so there is **no `MutexGuard` held across `.await`**. Registered in the Tauri builder's `invoke_handler`.
- `digest.rs` / `ask.rs`: **unchanged** — they obtain a provider through `make_provider`, so OpenRouter slots in transparently.

### 4. Frontend

- `src/lib/api.ts`: add `ModelInfo` type (`{ id: string; name: string }`) and `listOpenRouterModels(): Promise<ModelInfo[]>` wrapping the new command.
- `src/lib/modelFilter.ts` (new): pure `filterModels(list: ModelInfo[], query: string): ModelInfo[]` — case-insensitive substring match over `id` and `name`; empty/whitespace query returns the full list. Unit-tested with vitest.
- `src/lib/components/Settings.svelte`:
  - Provider dropdown becomes **`claude` + `openrouter`** only (remove the dead `openai`/`ollama` completion options — `ollama` remains in the embedding selector, untouched).
  - When `s.provider === "openrouter"`, render a model picker block:
    - On entering OpenRouter (and via an explicit "Refresh models" affordance), call `listOpenRouterModels()` once. Track `loadingModels` / `modelsError` / fetched `models`.
    - A neo-brutalist search input (`bind:value` to a local `query`) filters `models` via `filterModels`; render the matches as a clickable list; clicking sets `s.model`.
    - The existing free-text `model` input stays visible and editable, so arbitrary IDs work even if the fetch fails or the model isn't listed.
    - States: loading (Spinner), error (retry, in `var(--pink)`), empty-after-fetch, and the normal filtered list.

## Data flow

```
Settings UI (provider=openrouter, model=<id>, key in keychain)
   → submit_source / ask_question
      → make_provider(settings) → OpenRouterProvider
         → POST /chat/completions (Bearer key)
            → choices[0].message.content
```

Model list:

```
Settings UI mount / select openrouter / refresh
   → list_openrouter_models command
      → fetch_models → GET /models (keyless) → parse_models → ModelInfo[]
         → filterModels(query) in the browser
```

## Error handling

- `complete()`: network error → propagated; non-2xx → `anyhow!` with status + body; unparseable success → `anyhow!("unexpected OpenRouter response shape")`. Key never appears in any message.
- `fetch_models()` / `list_openrouter_models`: network or non-2xx → `Err(String)` surfaced to the UI as a retry-able error state. The endpoint is keyless, so no secret is involved.
- Frontend model picker degrades gracefully: a failed fetch leaves the free-text model input fully usable.

## Testing

**Rust (no live network):**
- `chat_body` builds the correct OpenAI shape (model, `max_tokens`, system + user messages in order).
- `parse_models` extracts `id`/`name` from sample `/models` JSON, defaults `name` to `id`, and skips entries lacking `id`.
- `make_provider` returns an OpenRouter provider for `"openrouter"` and still errors for unknown providers.

**Frontend (vitest, no live network):**
- `filterModels`: case-insensitive match on id and name; empty query returns all; no-match returns empty.

## Security

Touches sensitive areas: the **`provider/`** boundary (new outbound endpoint) and **`settings.rs`** (`make_provider`). Run the **`security-reviewer` agent before commit**, focused on:
- The Bearer API key is never logged, echoed, or included in error messages.
- The `/models` path sends no secret.
- Error bodies surfaced to the UI cannot leak the key.

## Out of scope / YAGNI

- Per-provider key storage (single shared slot retained).
- OpenRouter embeddings.
- Server-side model filtering (the list is small enough to fetch once and filter client-side).
- Persisting or caching the fetched model list across restarts.
