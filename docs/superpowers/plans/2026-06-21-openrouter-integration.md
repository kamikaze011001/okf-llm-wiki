# OpenRouter Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add OpenRouter as a second chat-completion provider alongside Claude, selectable in Settings, with a fetched + filterable model list.

**Architecture:** A new `OpenRouterProvider` implements the existing one-method `LlmProvider::complete` trait (OpenAI-compatible `/chat/completions`, Bearer auth). A keyless `/models` fetch feeds a new `list_openrouter_models` IPC command; the Settings UI filters that list client-side. Embeddings and the single-keychain-slot key model are untouched.

**Tech Stack:** Rust (Tauri 2, reqwest, serde_json, async-trait, tokio) + SvelteKit/Svelte 5 (legacy syntax) + vitest.

**Spec:** `docs/superpowers/specs/2026-06-21-openrouter-integration-design.md`

**Conventions / gotchas (read once):**
- Cargo is **not on PATH**: run as `$HOME/.cargo/bin/cargo`, from inside `src-tauri/`. Shell `cd` does not persist — use compound `cd src-tauri && …`.
- Conventional Commits; end every commit message **exactly** with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Keep `cargo clippy --all-targets` warning-free and `cargo fmt` clean before each commit.
- Svelte components here use **legacy syntax** (`on:click`, `export let`, `$:`, `bind:value`, `$store`) — NOT runes.
- **The API key must never be logged, echoed, or placed in an error message.**
- **Sensitive areas touched:** `src-tauri/src/core/provider/` and `settings.rs` (`make_provider`). A `security-reviewer` agent pass is REQUIRED before opening the PR (final step).

## File Structure

| File | New? | Responsibility |
|---|---|---|
| `src-tauri/src/core/provider/openrouter.rs` | new | `OpenRouterProvider` (`complete`), `ModelInfo`, `parse_models`, `fetch_models` |
| `src-tauri/src/core/provider/mod.rs` | modify | declare `pub mod openrouter;` |
| `src-tauri/src/core/settings.rs` | modify | `make_provider` "openrouter" arm |
| `src-tauri/src/commands.rs` | modify | `list_openrouter_models` command |
| `src-tauri/src/lib.rs` | modify | register the command in `invoke_handler` |
| `src/lib/modelFilter.ts` | new | pure `filterModels(list, query)` |
| `src/lib/modelFilter.test.ts` | new | vitest for `filterModels` |
| `src/lib/api.ts` | modify | `ModelInfo` type + `listOpenRouterModels` client |
| `src/lib/components/Settings.svelte` | modify | provider dropdown cleanup + model picker block |

---

## Task 1: OpenRouterProvider (complete)

**Files:**
- Create: `src-tauri/src/core/provider/openrouter.rs`
- Modify: `src-tauri/src/core/provider/mod.rs`

- [ ] **Step 1: Declare the module**

In `src-tauri/src/core/provider/mod.rs`, add after the existing `pub mod fake;` line:

```rust
pub mod openrouter;
```

- [ ] **Step 2: Write the failing test + minimal struct**

Create `src-tauri/src/core/provider/openrouter.rs` with the provider, the `chat_body` helper, and its test:

```rust
use super::LlmProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct OpenRouterProvider {
    pub api_key: String,
    pub model: String, // e.g. "openai/gpt-4o"
    pub client: reqwest::Client,
}

impl OpenRouterProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
    pub(crate) fn chat_body(&self, system: &str, user: &str) -> Value {
        json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        })
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("X-Title", "okf-llm-wiki")
            .json(&self.chat_body(system, user))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenRouter API error {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let v: Value = resp.json().await?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected OpenRouter response shape"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_openai_chat_body() {
        let p = OpenRouterProvider::new("k".into(), "openai/gpt-4o".into());
        let b = p.chat_body("be brief", "hi");
        assert_eq!(b["model"], "openai/gpt-4o");
        assert_eq!(b["max_tokens"], 4096);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][0]["content"], "be brief");
        assert_eq!(b["messages"][1]["role"], "user");
        assert_eq!(b["messages"][1]["content"], "hi");
    }
}
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test openrouter::tests::builds_openai_chat_body`
Expected: PASS (1 passed). Compiles cleanly.

- [ ] **Step 4: Lint + format**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt`
Expected: no warnings, no diff after fmt.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/provider/openrouter.rs src-tauri/src/core/provider/mod.rs
git commit -m "$(cat <<'EOF'
feat: add OpenRouterProvider implementing LlmProvider::complete

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Model listing (parse_models + fetch_models)

**Files:**
- Modify: `src-tauri/src/core/provider/openrouter.rs`

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/core/provider/openrouter.rs`, add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn parses_models_with_name_fallback_and_skips_missing_id() {
        let v = serde_json::json!({
            "data": [
                { "id": "openai/gpt-4o", "name": "GPT-4o" },
                { "id": "meta/llama-3" },          // no name -> falls back to id
                { "name": "ghost" }                // no id -> skipped
            ]
        });
        let models = parse_models(&v);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0], ModelInfo { id: "openai/gpt-4o".into(), name: "GPT-4o".into() });
        assert_eq!(models[1], ModelInfo { id: "meta/llama-3".into(), name: "meta/llama-3".into() });
    }

    #[test]
    fn parses_empty_when_no_data_array() {
        assert!(parse_models(&serde_json::json!({})).is_empty());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test openrouter::tests::parses`
Expected: FAIL to compile — `ModelInfo` and `parse_models` not found.

- [ ] **Step 3: Implement ModelInfo, parse_models, fetch_models**

Add `use serde::Serialize;` to the top imports of `openrouter.rs` (alongside the existing `use serde_json::{json, Value};`). Then add, after the `impl LlmProvider for OpenRouterProvider` block:

```rust
/// A single model offered by OpenRouter. `name` falls back to `id` when the API omits it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// Pure parse of the `/models` response shape: `{ "data": [ { "id", "name"? }, … ] }`.
/// Entries without an `id` are skipped; a missing `name` defaults to the `id`.
pub fn parse_models(v: &Value) -> Vec<ModelInfo> {
    let Some(arr) = v["data"].as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let name = m["name"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| id.clone());
            Some(ModelInfo { id, name })
        })
        .collect()
}

/// Fetch the (keyless) public model catalog from OpenRouter.
pub async fn fetch_models(client: &reqwest::Client) -> Result<Vec<ModelInfo>> {
    let resp = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "OpenRouter models error {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let v: Value = resp.json().await?;
    Ok(parse_models(&v))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test openrouter::tests::parses`
Expected: PASS (2 passed).

- [ ] **Step 5: Lint + format**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt`
Expected: no warnings, no diff.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/provider/openrouter.rs
git commit -m "$(cat <<'EOF'
feat: add OpenRouter model catalog parse + fetch

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Wire make_provider

**Files:**
- Modify: `src-tauri/src/core/settings.rs`

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/core/settings.rs`, add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn make_provider_supports_openrouter() {
        let s = Settings {
            provider: "openrouter".into(),
            ..Settings::default()
        };
        assert!(make_provider(&s).is_ok());
    }

    #[test]
    fn make_provider_rejects_unknown() {
        let s = Settings {
            provider: "nope".into(),
            ..Settings::default()
        };
        assert!(make_provider(&s).is_err());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test settings::tests::make_provider`
Expected: FAIL — `make_provider_supports_openrouter` fails (currently returns `Err` for `"openrouter"`).

- [ ] **Step 3: Add the openrouter arm**

In `src-tauri/src/core/settings.rs`, change the import line:

```rust
use crate::core::provider::{claude::ClaudeProvider, openrouter::OpenRouterProvider, LlmProvider};
```

and add an arm to `make_provider` before the `other =>` arm:

```rust
        "openrouter" => Ok(Arc::new(OpenRouterProvider::new(
            s.api_key.clone(),
            s.model.clone(),
        ))),
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test settings::tests::make_provider`
Expected: PASS (2 passed).

- [ ] **Step 5: Lint + format**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt`
Expected: no warnings, no diff.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/settings.rs
git commit -m "$(cat <<'EOF'
feat: route provider=openrouter through OpenRouterProvider

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: list_openrouter_models command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

> **No unit test:** this command is thin glue over `fetch_models`, which performs a live network call and cannot be exercised offline. Its testable logic (`parse_models`) is already covered by Task 2. Verification here is a clean compile + clippy.

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, extend the `use crate::core::{…}` block by adding `provider::openrouter::{fetch_models, ModelInfo},` (keep the list alphabetical-ish, e.g. right after the `links::…` line). Then add this command at the end of the file:

```rust
/// List OpenRouter's public model catalog for the Settings model picker.
/// The `/models` endpoint is keyless, so this takes no arguments and touches no
/// shared state — there is no `MutexGuard` held across the `.await`.
#[tauri::command]
pub async fn list_openrouter_models() -> Result<Vec<ModelInfo>, String> {
    fetch_models(&reqwest::Client::new())
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, add `commands::list_openrouter_models` to the `tauri::generate_handler![…]` list (add a comma after `commands::get_graph` and a new line):

```rust
            commands::get_graph,
            commands::list_openrouter_models
```

- [ ] **Step 3: Verify it compiles cleanly**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets`
Expected: no warnings, no errors.

- [ ] **Step 4: Confirm the full Rust suite is still green + format**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test && $HOME/.cargo/bin/cargo fmt`
Expected: all tests pass (existing suite + the new openrouter/settings tests), no fmt diff.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: add list_openrouter_models IPC command

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: filterModels helper (frontend, pure)

**Files:**
- Create: `src/lib/modelFilter.ts`
- Test: `src/lib/modelFilter.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/lib/modelFilter.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { filterModels } from "./modelFilter";
import type { ModelInfo } from "./api";

const models: ModelInfo[] = [
  { id: "openai/gpt-4o", name: "GPT-4o" },
  { id: "anthropic/claude-3.5-sonnet", name: "Claude 3.5 Sonnet" },
  { id: "meta-llama/llama-3-70b", name: "Llama 3 70B" },
];

describe("filterModels", () => {
  it("returns all when query is blank", () => expect(filterModels(models, "")).toEqual(models));
  it("returns all when query is whitespace", () => expect(filterModels(models, "   ")).toEqual(models));
  it("matches on id case-insensitively", () =>
    expect(filterModels(models, "OPENAI").map((m) => m.id)).toEqual(["openai/gpt-4o"]));
  it("matches on name case-insensitively", () =>
    expect(filterModels(models, "claude").map((m) => m.id)).toEqual(["anthropic/claude-3.5-sonnet"]));
  it("returns empty when nothing matches", () => expect(filterModels(models, "zzz")).toEqual([]));
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm run test -- modelFilter`
Expected: FAIL — cannot resolve `./modelFilter` (and `ModelInfo` not yet exported; that is added in Task 6, but this test only imports the *type*, so add it now in Step 3 OR proceed — see note).

> **Note on `ModelInfo`:** this test imports `ModelInfo` as a type from `./api`. To keep Task 5 self-contained, add the `ModelInfo` interface to `api.ts` now as part of Step 3 (the `listOpenRouterModels` client function is added in Task 6).

- [ ] **Step 3: Implement the helper (and the type it depends on)**

Add the `ModelInfo` interface to `src/lib/api.ts` after the `GraphData` interface line:

```ts
export interface ModelInfo { id: string; name: string; }
```

Create `src/lib/modelFilter.ts`:

```ts
import type { ModelInfo } from "./api";

/** Case-insensitive substring filter over a model's id and display name.
 *  A blank/whitespace query returns the list unchanged. */
export function filterModels(list: ModelInfo[], query: string): ModelInfo[] {
  const q = query.trim().toLowerCase();
  if (!q) return list;
  return list.filter(
    (m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q),
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm run test -- modelFilter`
Expected: PASS (5 passed).

- [ ] **Step 5: Commit**

```bash
git add src/lib/modelFilter.ts src/lib/modelFilter.test.ts src/lib/api.ts
git commit -m "$(cat <<'EOF'
feat: add filterModels helper + ModelInfo type

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Settings UI — provider cleanup + model picker

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/lib/components/Settings.svelte`

> No component test exists for `Settings.svelte` (matching the codebase — only pure helpers are unit-tested). The pure logic (`filterModels`) is covered in Task 5; this task is verified by `npm run check`, the full vitest run, and `npm run build`.

- [ ] **Step 1: Add the client function**

In `src/lib/api.ts`, add after the `getGraph` export line:

```ts
export const listOpenRouterModels = () => invoke<ModelInfo[]>("list_openrouter_models");
```

(The `ModelInfo` interface was added in Task 5.)

- [ ] **Step 2: Rewrite Settings.svelte**

Replace the entire contents of `src/lib/components/Settings.svelte` with:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { getSettings, setSettings, reindex, listOpenRouterModels, type Settings, type ModelInfo } from "$lib/api";
  import { filterModels } from "$lib/modelFilter";
  import Spinner from "$lib/components/Spinner.svelte";
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"", embed_provider:"hash", embed_model:"nomic-embed-text", ollama_url:"http://localhost:11434" };
  let saved = false;
  let error = "";
  let reindexing = false;
  let reindexError = "";

  // OpenRouter model picker state.
  let models: ModelInfo[] = [];
  let modelsLoaded = false;
  let loadingModels = false;
  let modelsError = "";
  let modelQuery = "";

  onMount(async () => { s = await getSettings(); });

  async function loadModels(){
    loadingModels = true;
    modelsError = "";
    try {
      models = await listOpenRouterModels();
      modelsLoaded = true;
    } catch (e) {
      modelsError = String(e);
    } finally {
      loadingModels = false;
    }
  }

  // Fetch the catalog the first time OpenRouter is selected. The `!modelsError`
  // guard stops an auto-retry loop after a failure (the Refresh button re-fetches).
  $: if (s.provider === "openrouter" && !modelsLoaded && !loadingModels && !modelsError) loadModels();
  $: filtered = filterModels(models, modelQuery);

  async function save(){
    error = "";
    try {
      await setSettings(s);
      saved = true;
      setTimeout(()=>saved=false, 1500);
    } catch (e) {
      error = String(e);
    }
  }
  async function runReindex(){
    reindexError = "";
    reindexing = true;
    try {
      await reindex();
    } catch (e) {
      reindexError = String(e);
    } finally {
      reindexing = false;
    }
  }
</script>
<section style="padding:32px;max-width:560px;margin:0 auto">
  <h1>Settings</h1>
  <div class="nb-card" style="display:grid;gap:10px">
    <label>Provider<select class="nb-input" bind:value={s.provider}><option>claude</option><option>openrouter</option></select></label>
    <label>Model<input class="nb-input" bind:value={s.model} placeholder={s.provider === "openrouter" ? "e.g. openai/gpt-4o" : "claude-opus-4-8"} /></label>
    {#if s.provider === "openrouter"}
      <div class="nb-card" style="background:var(--paper)">
        <div style="display:flex;justify-content:space-between;align-items:center;gap:8px">
          <strong>Browse models</strong>
          <button class="nb-btn" on:click={loadModels} disabled={loadingModels}>{loadingModels?"Loading…":"Refresh"}</button>
        </div>
        {#if loadingModels}
          <div style="margin-top:8px"><Spinner label="Fetching models…" /></div>
        {:else if modelsError}
          <p style="color:var(--pink);font-weight:700">⚠ {modelsError}</p>
        {:else if modelsLoaded}
          <input class="nb-input" style="width:100%;margin-top:8px" placeholder="Filter models…" bind:value={modelQuery} />
          {#if filtered.length}
            <ul style="list-style:none;margin:8px 0 0 0;padding:0;max-height:220px;overflow:auto">
              {#each filtered.slice(0, 100) as m (m.id)}
                <li>
                  <button class="nb-btn" style="width:100%;text-align:left;margin-top:4px;{s.model===m.id?'background:var(--blue);color:#fff':''}" on:click={() => (s.model = m.id)}>
                    <strong>{m.id}</strong>{#if m.name && m.name !== m.id} — {m.name}{/if}
                  </button>
                </li>
              {/each}
            </ul>
            {#if filtered.length > 100}<p style="margin-top:4px">Showing first 100 of {filtered.length}. Narrow your filter.</p>{/if}
          {:else}
            <p style="margin-top:8px">No models match “{modelQuery}”.</p>
          {/if}
        {/if}
      </div>
    {/if}
    <label>API key<input class="nb-input" type="password" bind:value={s.api_key} /></label>
    <label>Wiki folder<input class="nb-input" bind:value={s.wiki_path} placeholder="/Users/you/wiki" /></label>
    <label>Embedding<select class="nb-input" bind:value={s.embed_provider}><option value="hash">hash (offline)</option><option value="ollama">ollama</option></select></label>
    {#if s.embed_provider === "ollama"}
      <label>Ollama URL<input class="nb-input" bind:value={s.ollama_url} placeholder="http://localhost:11434" /></label>
      <label>Embedding model<input class="nb-input" bind:value={s.embed_model} placeholder="nomic-embed-text" /></label>
    {/if}
    <button class="nb-btn accent" on:click={save}>{saved?"Saved ✓":"Save"}</button>
    {#if error}<p style="color:var(--pink);font-weight:700">⚠ {error}</p>{/if}
    <button class="nb-btn" on:click={runReindex} disabled={reindexing}>{reindexing?"Reindexing…":"Reindex wiki"}</button>
    {#if reindexError}<p style="color:var(--pink);font-weight:700">⚠ {reindexError}</p>{/if}
  </div>
</section>
```

- [ ] **Step 3: Type-check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 4: Run the full frontend test suite**

Run: `npm run test`
Expected: all tests pass (existing 29 + the 5 new `filterModels` cases = 34).

- [ ] **Step 5: Build the SPA**

Run: `npm run build`
Expected: build succeeds (adapter-static → `build/`).

- [ ] **Step 6: Commit**

```bash
git add src/lib/api.ts src/lib/components/Settings.svelte
git commit -m "$(cat <<'EOF'
feat: OpenRouter model picker in Settings; drop dead provider options

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Final steps (after all tasks)

- [ ] **Security review (REQUIRED — sensitive areas):** Run the `security-reviewer` agent over the branch diff, focused on `provider/openrouter.rs`, `settings.rs` (`make_provider`), and `commands.rs`. Confirm: the Bearer API key never appears in logs or error messages; the `/models` request sends no secret; surfaced error bodies cannot leak the key. Address any findings before the PR.
- [ ] **Final review:** Dispatch a final code reviewer over the whole branch.
- [ ] **Full gate:** `cd src-tauri && $HOME/.cargo/bin/cargo test && $HOME/.cargo/bin/cargo clippy --all-targets` green; `npm run check` 0 errors; `npm run test` green; `npm run build` OK.
- [ ] **Finish the branch:** Use `superpowers:finishing-a-development-branch` to open the PR.

---

## Self-Review

**Spec coverage:**
- Provider (`OpenRouterProvider::complete`, OpenAI shape, Bearer + X-Title, error handling, key never logged) → Task 1. ✅
- `ModelInfo` + `parse_models` (skip missing id, name→id fallback) + `fetch_models` (keyless) → Task 2. ✅
- `make_provider` "openrouter" arm → Task 3. ✅
- `list_openrouter_models` command (no args, no lock-across-await) + registration → Task 4. ✅
- `filterModels` pure helper + vitest → Task 5. ✅
- `api.ts` (`ModelInfo`, `listOpenRouterModels`) → Tasks 5 + 6. ✅
- Settings UI: provider dropdown `claude`+`openrouter` (drop dead options), model picker with loading/error/empty/filtered states, free-text model input retained → Task 6. ✅
- Embeddings untouched (ollama stays in embed selector) → preserved verbatim in Task 6 rewrite. ✅
- Security review of sensitive areas → Final steps. ✅
- Out-of-scope items (per-provider keys, OR embeddings, server-side filter, caching) → not implemented. ✅

**Placeholder scan:** No TBD/TODO; every code step shows complete code; the two no-unit-test tasks (4, 6) state why and give concrete verification commands.

**Type consistency:** `ModelInfo { id, name }` identical in Rust (Task 2) and TS (Task 5). `filterModels(list, query)` signature matches between helper (Task 5) and caller (Task 6). `listOpenRouterModels()` name consistent (api.ts ↔ Settings.svelte). Command name `list_openrouter_models` consistent (commands.rs ↔ lib.rs ↔ api.ts invoke string).
