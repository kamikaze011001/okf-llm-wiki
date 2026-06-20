# M2 — Make Ask Genuinely Good: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hash-only retrieval index with a provider-agnostic embedding seam (Ollama backend + offline hash fallback), persist a content-hashed chunked index to disk, and chunk long pages so Ask retrieves and cites real passages.

**Architecture:** A new `Embedder` trait behind `make_embedder(&Settings)` decouples retrieval from any one embedding source. Pages are split into paragraph-aware chunks, embedded, and stored in a `PersistedIndex` (JSON, content-hashed per page for incremental re-embedding) that lives in the app-config dir next to `settings.json`. Startup only *deserializes* the index (never embeds / never hits the network); embedding happens in `submit_source`, `set_settings`, and a new `reindex` command, all async. Search embeds the query and ranks chunks by cosine similarity, skipping any chunk whose vector dimension no longer matches (stale-index guard).

**Tech Stack:** Rust + Tauri 2 core (`async_trait`, `reqwest`, `serde`/`serde_json`, `anyhow`), SvelteKit + Svelte 5 frontend. No new dependencies.

---

## Conventions for every task

- **Run all `cargo` commands from `src-tauri/`.** `cargo` is NOT on `PATH` — invoke it as `$HOME/.cargo/bin/cargo`. Shell `cd` does NOT persist between commands, so use a compound command, e.g.
  `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test embed::`
- **`core/` stays Tauri-free** (ADR-0001): no `tauri::` imports anywhere under `src-tauri/src/core/`.
- **Never hold a `MutexGuard` across an `.await`** in `commands.rs` — clone/drop the lock *before* awaiting.
- **API key stays in the OS keychain only** — never log it, never put it in `index.json`, never surface it in errors/UI.
- **Test temp-dir pattern:** a process-wide `AtomicU64` counter, names like `okf-<purpose>-{pid}-{n}`, `remove_dir_all` then `create_dir_all`. Match the existing pattern in `state.rs`/`store.rs`.
- **Security review gate (`security-reviewer` agent) BEFORE the commit** of any task that touches: `settings.rs`, `state.rs`, `store.rs`, `provider/`, or the new outbound HTTP boundary `core/embed/ollama.rs`, or the new on-disk writer `core/index_store.rs`. Tasks that need it are flagged **🔒 SECURITY REVIEW**.
- **Commits:** Conventional Commits, ending with the trailer:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  ```
- After a Rust task, also run `$HOME/.cargo/bin/cargo clippy --all-targets` (warning-free) and `$HOME/.cargo/bin/cargo fmt` before committing.

---

## Task ordering rationale

Tasks 1–7 are **additive** — the tree compiles and the full suite stays green after each, because the old sync `IndexEntry`/`build_index`/`search` and `LlmProvider::embed` remain in place. Task 8 is the **atomic switchover**: it changes `AppState.index`'s type and every consumer at once, then deletes the old retrieval pieces. Tasks 9–11 add the `reindex` command, the frontend, and remove the now-dead `LlmProvider::embed`.

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src-tauri/src/core/settings.rs` | Modify 🔒 | Add 3 non-secret embed fields + defaults |
| `src-tauri/src/core/config.rs` | Modify 🔒 | Persist the 3 embed fields in `PersistedSettings` |
| `src-tauri/src/core/retrieval.rs` | Modify | Keep `hash_embed`/`cosine`; add `chunk_body`; (Task 7) add async `search`; (Task 8) delete old sync `search`/`build_index`/`IndexEntry` |
| `src-tauri/src/core/embed/mod.rs` | Create | `Embedder` trait + `make_embedder` |
| `src-tauri/src/core/embed/hash.rs` | Create | `HashEmbedder` (offline default) |
| `src-tauri/src/core/embed/ollama.rs` | Create 🔒 | `OllamaEmbedder` (outbound HTTP) |
| `src-tauri/src/core/index_store.rs` | Create 🔒 | `Chunk`/`PageEntry`/`PersistedIndex`, `content_hash`, `load`/`save`/`index_path`, `rebuild_index`, `flatten` |
| `src-tauri/src/core/mod.rs` | Modify | Register `embed` + `index_store` modules |
| `src-tauri/src/core/ask.rs` | Modify | `ask(provider, question, hits: &[&Chunk])` |
| `src-tauri/src/core/provider/mod.rs` | Modify 🔒 | Remove `embed` from `LlmProvider` |
| `src-tauri/src/core/provider/claude.rs` | Modify 🔒 | Remove `embed` impl |
| `src-tauri/src/core/provider/fake.rs` | Modify 🔒 | Remove `embed` impl + its test assertion |
| `src-tauri/src/state.rs` | Modify 🔒 | `index: Mutex<PersistedIndex>` + `index_path` + `initial_index` loads persisted |
| `src-tauri/src/lib.rs` | Modify | Setup: resolve `index_path`, load persisted index, register `reindex` |
| `src-tauri/src/commands.rs` | Modify | async embed-aware `submit_source`/`ask_question`/`set_settings`; new `reindex` |
| `src-tauri/tests/commands_integration.rs` | Modify | Switch to `HashEmbedder` + `rebuild_index` + new `search`/`ask` |
| `src/lib/api.ts` | Modify | `reindex()` + 3 new `Settings` fields |
| `src/lib/api.test.ts` (or existing test file) | Modify | Assert `reindex` wrapper + Settings shape |
| `src/lib/components/Settings.svelte` (or current settings UI) | Modify | Embed provider/model/url inputs + Reindex button |

---

### Task 1: Settings — add embed fields (provider/model/ollama url)

**🔒 SECURITY REVIEW before commit** (touches `settings.rs` + `config.rs`).

**Files:**
- Modify: `src-tauri/src/core/settings.rs`
- Modify: `src-tauri/src/core/config.rs`

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/core/settings.rs`, inside the existing `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn defaults_to_offline_hash_embedder() {
    let s = Settings::default();
    assert_eq!(s.embed_provider, "hash");
    assert_eq!(s.embed_model, "nomic-embed-text");
    assert_eq!(s.ollama_url, "http://localhost:11434");
}

#[test]
fn embed_fields_survive_json_roundtrip() {
    let s = Settings {
        embed_provider: "ollama".into(),
        embed_model: "nomic-embed-text".into(),
        ollama_url: "http://localhost:11434".into(),
        ..Settings::default()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.embed_provider, "ollama");
    assert_eq!(back.embed_model, "nomic-embed-text");
    assert_eq!(back.ollama_url, "http://localhost:11434");
}
```

In `src-tauri/src/core/config.rs`, inside its `#[cfg(test)] mod tests`, extend the existing roundtrip test (or add a new one) to assert the embed fields persist. Add:

```rust
#[test]
fn persists_embed_fields() {
    let dir = cfg_tmp(); // existing helper in this module's tests
    let store = ConfigStore::new(dir.clone(), Box::new(MemSecretStore::default()));
    let mut s = Settings::default();
    s.embed_provider = "ollama".into();
    s.embed_model = "nomic-embed-text".into();
    s.ollama_url = "http://host:1234".into();
    store.save(&s).unwrap();
    let loaded = store.load();
    assert_eq!(loaded.embed_provider, "ollama");
    assert_eq!(loaded.embed_model, "nomic-embed-text");
    assert_eq!(loaded.ollama_url, "http://host:1234");
}
```

> If the config test module's tmp-dir helper or `MemSecretStore` has different names, match the existing names in that module.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test settings:: config::`
Expected: FAIL — `no field 'embed_provider' on type 'Settings'`.

- [ ] **Step 3: Add the fields to `Settings`**

In `settings.rs`, add three fields to the `Settings` struct (with `#[serde(default = ...)]` so older persisted JSON without them still deserializes), and matching `Default`:

```rust
// in the Settings struct definition:
#[serde(default = "default_embed_provider")]
pub embed_provider: String,
#[serde(default = "default_embed_model")]
pub embed_model: String,
#[serde(default = "default_ollama_url")]
pub ollama_url: String,
```

Add these free functions near the struct:

```rust
fn default_embed_provider() -> String { "hash".into() }
fn default_embed_model() -> String { "nomic-embed-text".into() }
fn default_ollama_url() -> String { "http://localhost:11434".into() }
```

Update the `Default for Settings` impl to set:

```rust
embed_provider: default_embed_provider(),
embed_model: default_embed_model(),
ollama_url: default_ollama_url(),
```

> Do NOT touch the hand-written `Debug` impl — it must keep redacting `api_key`. The new fields are non-secret; let `Debug` print them normally (add them to the `Debug` impl's field list if it enumerates fields explicitly).

- [ ] **Step 4: Persist the fields in `config.rs`**

In `config.rs`, add the three fields to `PersistedSettings` (the api-key-free struct):

```rust
pub embed_provider: String,
pub embed_model: String,
pub ollama_url: String,
```

In `save` (where it maps `Settings` → `PersistedSettings`), add:
```rust
embed_provider: settings.embed_provider.clone(),
embed_model: settings.embed_model.clone(),
ollama_url: settings.ollama_url.clone(),
```

In `load` (where it maps `PersistedSettings` → `Settings`), add:
```rust
embed_provider: persisted.embed_provider,
embed_model: persisted.embed_model,
ollama_url: persisted.ollama_url,
```

> If `PersistedSettings` derives `Default`/serde defaults, give the three fields `#[serde(default = "...")]` too so configs written before M2 still load. Reuse the same default fns by re-exporting or duplicating; a corrupt/old config must still default to `"hash"`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test settings:: config::`
Expected: PASS (including the existing `roundtrips_json` and `debug_redacts_api_key`).

- [ ] **Step 6: Lint + format, then commit (after 🔒 security review)**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/settings.rs src-tauri/src/core/config.rs
git commit -m "feat: add embed provider/model/url settings fields

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `chunk_body` — paragraph-aware chunking

**Files:**
- Modify: `src-tauri/src/core/retrieval.rs`

- [ ] **Step 1: Write the failing tests**

In `retrieval.rs`'s `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn empty_body_yields_no_chunks() {
    assert!(chunk_body("").is_empty());
    assert!(chunk_body("   \n\n  ").is_empty());
}

#[test]
fn short_body_is_one_chunk() {
    let chunks = chunk_body("para one\n\npara two");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].contains("para one"));
    assert!(chunks[0].contains("para two"));
}

#[test]
fn packs_paragraphs_up_to_limit_without_splitting() {
    let p = "x".repeat(500);
    // two 500-char paragraphs cannot share an 800-char chunk -> two chunks
    let chunks = chunk_body(&format!("{p}\n\n{p}"));
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].chars().count(), 500);
    assert_eq!(chunks[1].chars().count(), 500);
}

#[test]
fn oversized_paragraph_becomes_its_own_chunk() {
    let big = "y".repeat(2000);
    let chunks = chunk_body(&big);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].chars().count(), 2000);
}

#[test]
fn headings_start_new_paragraph_boundaries() {
    let body = "intro text\n\n## Section\n\nbody text";
    let chunks = chunk_body(body);
    assert_eq!(chunks.len(), 1); // all fits under 800
    assert!(chunks[0].contains("## Section"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test retrieval::tests::`
Expected: FAIL — `cannot find function 'chunk_body'`.

- [ ] **Step 3: Implement `chunk_body`**

Add to `retrieval.rs`:

```rust
/// Target maximum size (in characters) for a single chunk.
pub const MAX_CHUNK_CHARS: usize = 800;

/// Split a page body into retrieval chunks.
///
/// Paragraphs are delimited by blank lines. Markdown headings (`#`-prefixed lines)
/// also start a new paragraph. Paragraphs are greedily packed into chunks up to
/// `MAX_CHUNK_CHARS`; a paragraph is never split across chunks. A single paragraph
/// longer than the limit becomes its own (oversized) chunk. An empty/whitespace-only
/// body yields no chunks.
pub fn chunk_body(body: &str) -> Vec<String> {
    let paragraphs = split_paragraphs(body);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for para in paragraphs {
        if current.is_empty() {
            current = para;
        } else if current.chars().count() + 2 + para.chars().count() <= MAX_CHUNK_CHARS {
            current.push_str("\n\n");
            current.push_str(&para);
        } else {
            chunks.push(std::mem::take(&mut current));
            current = para;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Group raw lines into trimmed paragraphs. Blank lines separate paragraphs; a
/// heading line (starts with `#`) forces a boundary before itself.
fn split_paragraphs(body: &str) -> Vec<String> {
    let mut paras: Vec<String> = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    let flush = |buf: &mut Vec<&str>, paras: &mut Vec<String>| {
        if !buf.is_empty() {
            let joined = buf.join("\n");
            let trimmed = joined.trim();
            if !trimmed.is_empty() {
                paras.push(trimmed.to_string());
            }
            buf.clear();
        }
    };
    for line in body.lines() {
        if line.trim().is_empty() {
            flush(&mut buf, &mut paras);
        } else if line.trim_start().starts_with('#') {
            flush(&mut buf, &mut paras);
            buf.push(line);
            flush(&mut buf, &mut paras);
        } else {
            buf.push(line);
        }
    }
    flush(&mut buf, &mut paras);
    paras
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test retrieval::tests::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/retrieval.rs
git commit -m "feat: add paragraph-aware chunk_body to retrieval

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `Embedder` trait + `HashEmbedder` + `make_embedder`

**Files:**
- Create: `src-tauri/src/core/embed/mod.rs`
- Create: `src-tauri/src/core/embed/hash.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Register the module**

In `src-tauri/src/core/mod.rs`, add (keep the list alphabetical):

```rust
pub mod embed;
```
(insert after `pub mod digest;`)

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/core/embed/mod.rs` with a test module first (implementation comes in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::Settings;

    #[tokio::test]
    async fn hash_embedder_is_deterministic_and_normalized() {
        let e = HashEmbedder;
        let a = e.embed("vitamin d sleep").await.unwrap();
        let b = e.embed("vitamin d sleep").await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 256);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4 || norm == 0.0);
        assert_eq!(e.id(), "hash-fnv-256");
    }

    #[test]
    fn make_embedder_selects_hash_by_default() {
        let s = Settings::default();
        let e = make_embedder(&s).unwrap();
        assert_eq!(e.id(), "hash-fnv-256");
    }

    #[test]
    fn make_embedder_rejects_unknown_provider() {
        let s = Settings { embed_provider: "nope".into(), ..Settings::default() };
        assert!(make_embedder(&s).is_err());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test embed::`
Expected: FAIL to compile — `HashEmbedder`/`make_embedder` undefined.

- [ ] **Step 4: Implement the trait, `HashEmbedder`, and `make_embedder`**

Put the implementation at the top of `src-tauri/src/core/embed/mod.rs` (above the test module):

```rust
use crate::core::settings::Settings;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;

pub mod hash;
pub mod ollama; // added in Task 4; declare now so the module tree is stable

pub use hash::HashEmbedder;
pub use ollama::OllamaEmbedder;

/// Turns text into a dense vector. Async + fallible because real backends do network I/O.
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Stable identifier of this embedder+model. Used to detect when a persisted
    /// index was built by a different embedder and must be fully rebuilt.
    fn id(&self) -> String;
}

/// Construct the embedder selected in `Settings`. Pure construction — no network call.
pub fn make_embedder(s: &Settings) -> Result<Arc<dyn Embedder>> {
    match s.embed_provider.as_str() {
        "hash" => Ok(Arc::new(HashEmbedder)),
        "ollama" => Ok(Arc::new(OllamaEmbedder::new(
            s.ollama_url.clone(),
            s.embed_model.clone(),
        ))),
        other => Err(anyhow!("embed provider '{other}' not supported (use 'hash' or 'ollama')")),
    }
}
```

> `make_embedder` references `OllamaEmbedder::new`, created in Task 4. To keep Task 3 compiling on its own, create `src-tauri/src/core/embed/ollama.rs` now as a minimal stub and flesh it out in Task 4:
> ```rust
> pub struct OllamaEmbedder { base_url: String, model: String }
> impl OllamaEmbedder {
>     pub fn new(base_url: String, model: String) -> Self { Self { base_url, model } }
> }
> ```
> (No `Embedder` impl yet — Task 4 adds it. `make_embedder`'s `"ollama"` arm won't be exercised until then; that's fine.)
>
> Wait — `Arc::new(OllamaEmbedder::new(..))` requires `OllamaEmbedder: Embedder` to coerce to `Arc<dyn Embedder>`. So the `Embedder` impl MUST exist for the code to compile. Therefore: do NOT stub. Implement the full `OllamaEmbedder` (Task 4's Step 4 code) as part of THIS task's compile, but keep Task 4 for its dedicated tests. Simplest: **merge the `OllamaEmbedder` struct+impl into this step** using the code from Task 4 Step 4, and let Task 4 add only the tests. If you prefer strict separation, temporarily make the `"ollama"` arm `Err(anyhow!("ollama not yet wired"))` here and switch it to construct `OllamaEmbedder` in Task 4. **Choose the `Err` stub** to keep tasks independent: the unknown-provider test still passes, and the default-hash path is unaffected.

Create `src-tauri/src/core/embed/hash.rs`:

```rust
use super::Embedder;
use crate::core::retrieval::hash_embed;
use anyhow::Result;
use async_trait::async_trait;

/// Offline, deterministic fallback embedder over the existing FNV hashing scheme.
pub struct HashEmbedder;

#[async_trait]
impl Embedder for HashEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(hash_embed(text))
    }
    fn id(&self) -> String {
        "hash-fnv-256".to_string()
    }
}
```

Apply the **`Err` stub** decision: in `mod.rs`, replace the `"ollama"` arm with:
```rust
"ollama" => Err(anyhow!("ollama embedder wired in a later step")),
```
and remove the `pub mod ollama;` / `pub use ollama::OllamaEmbedder;` lines for now (Task 4 re-adds them). `hash_embed` is already `pub` in `retrieval.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test embed::`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/embed/ src-tauri/src/core/mod.rs
git commit -m "feat: add Embedder trait, HashEmbedder, and make_embedder

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `OllamaEmbedder` (outbound HTTP)

**🔒 SECURITY REVIEW before commit** (new outbound network boundary).

**Files:**
- Create: `src-tauri/src/core/embed/ollama.rs`
- Modify: `src-tauri/src/core/embed/mod.rs` (re-add module + wire `make_embedder`)

- [ ] **Step 1: Write the failing test (construction + id only)**

Network calls aren't unit-tested (no live Ollama in CI). Test construction and `id()` shape. In `ollama.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_encodes_model() {
        let e = OllamaEmbedder::new("http://localhost:11434".into(), "nomic-embed-text".into());
        assert_eq!(e.id(), "ollama:nomic-embed-text");
    }
}
```

Also add to `embed/mod.rs` tests:

```rust
#[test]
fn make_embedder_selects_ollama() {
    let s = Settings {
        embed_provider: "ollama".into(),
        embed_model: "nomic-embed-text".into(),
        ollama_url: "http://localhost:11434".into(),
        ..Settings::default()
    };
    let e = make_embedder(&s).unwrap();
    assert_eq!(e.id(), "ollama:nomic-embed-text");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test embed::`
Expected: FAIL — `OllamaEmbedder` missing / `make_embedder` ollama arm returns `Err`.

- [ ] **Step 3: Implement `OllamaEmbedder`**

Create `src-tauri/src/core/embed/ollama.rs`:

```rust
use super::Embedder;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Embeds text via a local Ollama server's `/api/embeddings` endpoint.
///
/// Targets keyless localhost (no auth header). Any non-2xx response or transport
/// error surfaces as `Err` so the UI can tell the user Ollama is unreachable.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Vec<f32>,
}

impl OllamaEmbedder {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&EmbedRequest { model: &self.model, prompt: text })
            .send()
            .await
            .context("requesting embedding from Ollama")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama API error {status}: {body}"));
        }
        let parsed: EmbedResponse = resp.json().await.context("parsing Ollama embedding response")?;
        if parsed.embedding.is_empty() {
            return Err(anyhow!("Ollama returned an empty embedding for model '{}'", self.model));
        }
        Ok(parsed.embedding)
    }
    fn id(&self) -> String {
        format!("ollama:{}", self.model)
    }
}
```

- [ ] **Step 4: Re-wire `make_embedder` in `embed/mod.rs`**

Re-add near the top:
```rust
pub mod ollama;
pub use ollama::OllamaEmbedder;
```
Replace the `"ollama"` arm:
```rust
"ollama" => Ok(Arc::new(OllamaEmbedder::new(
    s.ollama_url.clone(),
    s.embed_model.clone(),
))),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test embed::`
Expected: PASS.

- [ ] **Step 6: Lint + format, then commit (after 🔒 security review)**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/embed/ollama.rs src-tauri/src/core/embed/mod.rs
git commit -m "feat: add OllamaEmbedder backend over /api/embeddings

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `index_store` — types + `content_hash` + `load`/`save`/`index_path`/`flatten`

**🔒 SECURITY REVIEW before commit** (new on-disk writer; atomic IO like `store.rs`).

**Files:**
- Create: `src-tauri/src/core/index_store.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Register the module**

In `core/mod.rs` add (keep alphabetical, after `pub mod fetch;`):
```rust
pub mod index_store;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/core/index_store.rs` with a test module (impl in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn idx_tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-idx-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn content_hash_is_stable_and_field_sensitive() {
        assert_eq!(content_hash("T", "body"), content_hash("T", "body"));
        assert_ne!(content_hash("T", "body"), content_hash("T", "body2"));
        assert_ne!(content_hash("T", "body"), content_hash("T2", "body"));
        // boundary: title+body vs shifted boundary must differ (NUL separator)
        assert_ne!(content_hash("ab", "c"), content_hash("a", "bc"));
    }

    #[test]
    fn index_path_is_index_json_in_dir() {
        let p = index_path(std::path::Path::new("/some/dir"));
        assert!(p.ends_with("index.json"));
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = idx_tmp();
        let idx = load(&index_path(&dir));
        assert_eq!(idx.embedder_id, "");
        assert!(idx.pages.is_empty());
    }

    #[test]
    fn load_corrupt_returns_default() {
        let dir = idx_tmp();
        let path = index_path(&dir);
        std::fs::write(&path, b"{not json").unwrap();
        let idx = load(&path);
        assert_eq!(idx, PersistedIndex::default());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = idx_tmp();
        let path = index_path(&dir);
        let mut idx = PersistedIndex { embedder_id: "hash-fnv-256".into(), ..Default::default() };
        idx.pages.insert(
            "concepts/a.md".into(),
            PageEntry {
                content_hash: 42,
                chunks: vec![Chunk {
                    path: "concepts/a.md".into(),
                    chunk_id: 0,
                    text: "hello".into(),
                    vector: vec![0.1, 0.2],
                }],
            },
        );
        save(&path, &idx).unwrap();
        assert_eq!(load(&path), idx);
    }

    #[test]
    fn flatten_collects_all_chunks_in_page_order() {
        let mut idx = PersistedIndex::default();
        idx.pages.insert("b.md".into(), PageEntry { content_hash: 1, chunks: vec![
            Chunk { path: "b.md".into(), chunk_id: 0, text: "b0".into(), vector: vec![] },
        ]});
        idx.pages.insert("a.md".into(), PageEntry { content_hash: 1, chunks: vec![
            Chunk { path: "a.md".into(), chunk_id: 0, text: "a0".into(), vector: vec![] },
            Chunk { path: "a.md".into(), chunk_id: 1, text: "a1".into(), vector: vec![] },
        ]});
        let flat = flatten(&idx);
        // BTreeMap orders keys: a.md before b.md
        let texts: Vec<&str> = flat.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["a0", "a1", "b0"]);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test index_store::`
Expected: FAIL to compile — types/functions undefined.

- [ ] **Step 4: Implement the types and functions**

At the top of `index_store.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One embedded passage of a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub path: String,
    pub chunk_id: usize,
    /// The raw chunk text (what we show / feed to the LLM as context).
    pub text: String,
    /// The embedding vector (may have any dimension; mismatches are skipped at search time).
    pub vector: Vec<f32>,
}

/// All chunks for one page plus the content hash used to detect changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageEntry {
    pub content_hash: u64,
    pub chunks: Vec<Chunk>,
}

/// The full persisted retrieval index. `embedder_id` records which embedder built it
/// so a change of embedder forces a full rebuild.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistedIndex {
    pub embedder_id: String,
    pub pages: BTreeMap<String, PageEntry>,
}

/// FNV-1a hash of `title \0 body`. The NUL separator keeps `("ab","c")` distinct
/// from `("a","bc")`.
pub fn content_hash(title: &str, body: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3); // FNV prime
        }
    };
    mix(title.as_bytes());
    mix(&[0u8]);
    mix(body.as_bytes());
    h
}

/// Path of the index file inside the app's data directory.
pub fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("index.json")
}

/// Load the persisted index. A missing or corrupt file yields an empty index — the
/// app must always launch, and a bad index just means "no hits until next rebuild".
pub fn load(path: &Path) -> PersistedIndex {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => PersistedIndex::default(),
    }
}

/// Atomically write the index (temp file + rename), creating the parent dir if needed.
pub fn save(path: &Path, idx: &PersistedIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating index dir {}", parent.display()))?;
    }
    let json = serde_json::to_string(idx).context("serializing index")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Borrow every chunk across all pages, in `BTreeMap` (path-sorted) order.
pub fn flatten(idx: &PersistedIndex) -> Vec<&Chunk> {
    idx.pages.values().flat_map(|p| p.chunks.iter()).collect()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test index_store::`
Expected: PASS (6 tests).

- [ ] **Step 6: Lint + format, then commit (after 🔒 security review)**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/index_store.rs src-tauri/src/core/mod.rs
git commit -m "feat: add persisted index store (types, content_hash, load/save)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: `rebuild_index` — incremental, content-hashed re-embedding

**Files:**
- Modify: `src-tauri/src/core/index_store.rs`

- [ ] **Step 1: Write the failing tests**

Add to `index_store.rs`'s test module. A local counting fake embedder verifies that unchanged pages are NOT re-embedded:

```rust
use crate::core::embed::Embedder;
use crate::core::page::{Frontmatter, Page};
use crate::core::store::OkfStore;
use async_trait::async_trait;
use std::collections::BTreeMap as TestBTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

struct CountingEmbedder {
    id: String,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl Embedder for CountingEmbedder {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        self.calls.fetch_add(1, AtomicOrdering::SeqCst);
        // deterministic 2-dim vector from text length so vectors are stable
        let n = text.chars().count() as f32;
        Ok(vec![n, n / 2.0])
    }
    fn id(&self) -> String {
        self.id.clone()
    }
}

fn write_page(store: &OkfStore, path: &str, title: &str, body: &str) {
    store.write_page(&Page {
        path: path.into(),
        frontmatter: Frontmatter {
            type_: "Concept".into(),
            title: Some(title.into()),
            description: None,
            tags: vec![],
            resource: None,
            timestamp: None,
            note: None,
            extra: TestBTreeMap::new(),
        },
        body: body.into(),
    }).unwrap();
}

#[tokio::test]
async fn rebuild_embeds_each_chunk_of_each_page() {
    let dir = idx_tmp();
    let store = OkfStore::new(&dir);
    write_page(&store, "concepts/a.md", "Alpha", "one\n\ntwo");
    let calls = Arc::new(AtomicUsize::new(0));
    let e = CountingEmbedder { id: "count".into(), calls: calls.clone() };
    let idx = rebuild_index(&store, &e, &PersistedIndex::default()).await.unwrap();
    assert_eq!(idx.embedder_id, "count");
    assert_eq!(idx.pages.len(), 1);
    let entry = idx.pages.get("concepts/a.md").unwrap();
    assert_eq!(entry.chunks.len(), 1); // "one\n\ntwo" fits one chunk
    assert!(calls.load(AtomicOrdering::SeqCst) >= 1);
}

#[tokio::test]
async fn rebuild_reuses_unchanged_pages_when_embedder_matches() {
    let dir = idx_tmp();
    let store = OkfStore::new(&dir);
    write_page(&store, "concepts/a.md", "Alpha", "body text");
    let calls = Arc::new(AtomicUsize::new(0));
    let e = CountingEmbedder { id: "count".into(), calls: calls.clone() };
    let first = rebuild_index(&store, &e, &PersistedIndex::default()).await.unwrap();
    let after_first = calls.load(AtomicOrdering::SeqCst);
    // No content change, same embedder id -> zero new embed calls.
    let second = rebuild_index(&store, &e, &first).await.unwrap();
    assert_eq!(calls.load(AtomicOrdering::SeqCst), after_first);
    assert_eq!(first, second);
}

#[tokio::test]
async fn rebuild_full_when_embedder_id_changes() {
    let dir = idx_tmp();
    let store = OkfStore::new(&dir);
    write_page(&store, "concepts/a.md", "Alpha", "body text");
    let calls = Arc::new(AtomicUsize::new(0));
    let e1 = CountingEmbedder { id: "v1".into(), calls: calls.clone() };
    let first = rebuild_index(&store, &e1, &PersistedIndex::default()).await.unwrap();
    let baseline = calls.load(AtomicOrdering::SeqCst);
    let e2 = CountingEmbedder { id: "v2".into(), calls: calls.clone() };
    let _ = rebuild_index(&store, &e2, &first).await.unwrap();
    // id mismatch -> re-embed everything
    assert!(calls.load(AtomicOrdering::SeqCst) > baseline);
}

#[tokio::test]
async fn empty_body_page_embeds_the_title() {
    let dir = idx_tmp();
    let store = OkfStore::new(&dir);
    write_page(&store, "concepts/e.md", "OnlyTitle", "");
    let calls = Arc::new(AtomicUsize::new(0));
    let e = CountingEmbedder { id: "count".into(), calls: calls.clone() };
    let idx = rebuild_index(&store, &e, &PersistedIndex::default()).await.unwrap();
    let entry = idx.pages.get("concepts/e.md").unwrap();
    assert_eq!(entry.chunks.len(), 1);
    assert_eq!(entry.chunks[0].text, "OnlyTitle");
}

#[tokio::test]
async fn rebuild_drops_removed_pages() {
    let dir = idx_tmp();
    let store = OkfStore::new(&dir);
    write_page(&store, "concepts/a.md", "Alpha", "a body");
    write_page(&store, "concepts/b.md", "Beta", "b body");
    let calls = Arc::new(AtomicUsize::new(0));
    let e = CountingEmbedder { id: "count".into(), calls: calls.clone() };
    let first = rebuild_index(&store, &e, &PersistedIndex::default()).await.unwrap();
    assert_eq!(first.pages.len(), 2);
    std::fs::remove_file(dir.join("concepts/b.md")).unwrap();
    let second = rebuild_index(&store, &e, &first).await.unwrap();
    assert_eq!(second.pages.len(), 1);
    assert!(second.pages.contains_key("concepts/a.md"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test index_store::`
Expected: FAIL — `rebuild_index` undefined.

- [ ] **Step 3: Implement `rebuild_index`**

Add to `index_store.rs` (outside the test module). Note the imports it needs at the top of the file:

```rust
use crate::core::embed::Embedder;
use crate::core::retrieval::chunk_body;
use crate::core::store::OkfStore;
```

```rust
/// Rebuild the persisted index from the store, reusing unchanged pages when the
/// embedder is unchanged.
///
/// For each page: compute `content_hash(title, body)`. If `prev` was built by the
/// same embedder and holds a matching hash for this path, reuse its chunks verbatim
/// (no embedding). Otherwise chunk the body (`chunk_body`); an empty body falls back
/// to a single title-only chunk. Each chunk is embedded as `"{title}\n\n{chunk}"`,
/// but the RAW chunk text is what we store. Pages absent from the store are dropped.
pub async fn rebuild_index(
    store: &OkfStore,
    embedder: &dyn Embedder,
    prev: &PersistedIndex,
) -> Result<PersistedIndex> {
    let reuse = prev.embedder_id == embedder.id();
    let mut pages: BTreeMap<String, PageEntry> = BTreeMap::new();

    for path in store.list_pages()? {
        let page = store.read_page(&path)?;
        let title = page.frontmatter.title.clone().unwrap_or_default();
        let hash = content_hash(&title, &page.body);

        if reuse {
            if let Some(existing) = prev.pages.get(&path) {
                if existing.content_hash == hash {
                    pages.insert(path.clone(), existing.clone());
                    continue;
                }
            }
        }

        let mut texts = chunk_body(&page.body);
        if texts.is_empty() {
            texts.push(title.clone());
        }
        let mut chunks = Vec::with_capacity(texts.len());
        for (chunk_id, text) in texts.into_iter().enumerate() {
            let embed_input = format!("{title}\n\n{text}");
            let vector = embedder.embed(&embed_input).await?;
            chunks.push(Chunk { path: path.clone(), chunk_id, text, vector });
        }
        pages.insert(path.clone(), PageEntry { content_hash: hash, chunks });
    }

    Ok(PersistedIndex { embedder_id: embedder.id(), pages })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test index_store::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/index_store.rs
git commit -m "feat: add incremental rebuild_index for persisted index

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Async `search` over the persisted index (coexists with old sync search)

**Files:**
- Modify: `src-tauri/src/core/retrieval.rs`

> The old sync `search(query, &[IndexEntry], k)` stays for now (Task 8 removes it). To avoid a name clash, the new function is also called `search` but with a different signature — Rust does NOT allow two functions of the same name in one module. So name the new one `search_index` here; Task 8 renames it to `search` after deleting the old one. (Keeping both compiling is the priority.)

- [ ] **Step 1: Write the failing test**

Add to `retrieval.rs` test module:

```rust
#[tokio::test]
async fn search_index_ranks_by_cosine_and_skips_dim_mismatch() {
    use crate::core::embed::HashEmbedder;
    use crate::core::index_store::{Chunk, PageEntry, PersistedIndex};

    let e = HashEmbedder;
    let mut idx = PersistedIndex { embedder_id: e.id(), ..Default::default() };
    // relevant chunk: embed the same phrase we will query
    let good = hash_embed("vitamin d improves sleep");
    // a stale chunk with the WRONG dimension must be skipped, not panic
    let stale = vec![0.5f32; 8];
    idx.pages.insert("concepts/vd.md".into(), PageEntry {
        content_hash: 1,
        chunks: vec![
            Chunk { path: "concepts/vd.md".into(), chunk_id: 0, text: "vitamin d improves sleep".into(), vector: good },
            Chunk { path: "concepts/vd.md".into(), chunk_id: 1, text: "stale".into(), vector: stale },
        ],
    });
    idx.pages.insert("concepts/rust.md".into(), PageEntry {
        content_hash: 1,
        chunks: vec![
            Chunk { path: "concepts/rust.md".into(), chunk_id: 0, text: "rust tauri desktop".into(), vector: hash_embed("rust tauri desktop") },
        ],
    });

    let hits = search_index(&e, "vitamin d improves sleep", &idx, 2).await.unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].path, "concepts/vd.md");
    assert_eq!(hits[0].chunk_id, 0);
    // the dim-mismatched "stale" chunk must never be returned
    assert!(hits.iter().all(|h| h.text != "stale"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test retrieval::tests::search_index`
Expected: FAIL — `search_index` undefined.

- [ ] **Step 3: Implement `search_index`**

Add to `retrieval.rs` (it already has `use anyhow::Result;` and `cosine`):

```rust
use crate::core::embed::Embedder;
use crate::core::index_store::{flatten, Chunk, PersistedIndex};

/// Embed the query and return the top-`k` chunks by cosine similarity.
///
/// Chunks whose stored vector dimension differs from the freshly embedded query
/// (a stale index built by a different embedder) are skipped rather than scored —
/// no panic, they simply don't match. Ties break by `(path, chunk_id)` for stable
/// ordering.
pub async fn search_index<'a>(
    embedder: &dyn Embedder,
    query: &str,
    index: &'a PersistedIndex,
    k: usize,
) -> Result<Vec<&'a Chunk>> {
    let q = embedder.embed(query).await?;
    let mut scored: Vec<(f32, &Chunk)> = flatten(index)
        .into_iter()
        .filter(|c| c.vector.len() == q.len())
        .map(|c| (cosine(&q, &c.vector), c))
        .collect();
    scored.sort_by(|a, b| {
        b.0
            .partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| (a.1.path.as_str(), a.1.chunk_id).cmp(&(b.1.path.as_str(), b.1.chunk_id)))
    });
    Ok(scored.into_iter().take(k).map(|(_, c)| c).collect())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test retrieval::`
Expected: PASS (old sync `search` tests still pass too).

- [ ] **Step 5: Commit**

```bash
cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
git add src-tauri/src/core/retrieval.rs
git commit -m "feat: add async search_index over persisted chunk index

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Switchover — wire the persisted async index through state, ask, lib, commands; delete old sync retrieval

**🔒 SECURITY REVIEW before commit** (touches `state.rs`).

This is the atomic cutover. After it, the tree is green again and the old `IndexEntry`/`build_index`/sync `search` are gone.

**Files:**
- Modify: `src-tauri/src/core/ask.rs`
- Modify: `src-tauri/src/core/retrieval.rs` (delete old pieces; rename `search_index` → `search`)
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/tests/commands_integration.rs`

- [ ] **Step 1: Update `ask.rs` (signature + test)**

Replace the body of `ask.rs` with:

```rust
use crate::core::index_store::Chunk;
use crate::core::provider::LlmProvider;
use anyhow::Result;

pub struct Answer {
    pub text: String,
    pub citations: Vec<String>,
}

/// Ask the LLM to answer `question` grounded ONLY in the pre-retrieved `hits`.
/// Citations are the hit page paths, de-duplicated, preserving rank order.
pub async fn ask(provider: &dyn LlmProvider, question: &str, hits: &[&Chunk]) -> Result<Answer> {
    let context = hits
        .iter()
        .map(|h| format!("[{}]\n{}", h.path, h.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut citations: Vec<String> = Vec::new();
    for h in hits {
        if !citations.contains(&h.path) {
            citations.push(h.path.clone());
        }
    }

    let system = "Answer ONLY from the provided wiki context. Cite the page paths you used in [brackets]. If the context does not contain the answer, say you don't know.";
    let user = format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}");
    let text = provider.complete(system, &user).await?;
    Ok(Answer { text, citations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::index_store::Chunk;
    use crate::core::provider::fake::FakeProvider;

    #[tokio::test]
    async fn answers_from_hits_and_dedupes_citations() {
        let c0 = Chunk { path: "concepts/vd.md".into(), chunk_id: 0, text: "Vitamin D helps sleep.".into(), vector: vec![] };
        let c1 = Chunk { path: "concepts/vd.md".into(), chunk_id: 1, text: "Take it in the morning.".into(), vector: vec![] };
        let hits: Vec<&Chunk> = vec![&c0, &c1];
        let p = FakeProvider { reply: "Morning dose [concepts/vd.md]".into() };
        let a = ask(&p, "when to take vitamin d", &hits).await.unwrap();
        assert!(a.text.contains("Morning"));
        // two hits, same page -> one citation
        assert_eq!(a.citations, vec!["concepts/vd.md".to_string()]);
    }
}
```

> Remove the old `ask.rs` test that built `IndexEntry` and the `use crate::core::retrieval::{search, IndexEntry}` import.

- [ ] **Step 2: Delete old retrieval pieces; rename `search_index` → `search`**

In `retrieval.rs`:
- Delete the `IndexEntry` struct, the old sync `pub fn search(query, entries, k)`, and `pub fn build_index(store)`.
- Delete the old retrieval tests that referenced `IndexEntry`/`build_index` (the ones at lines ~73–139 in the pre-M2 file).
- Rename `search_index` → `search` (signature unchanged) and update the Task-7 test name `search_index_...` → `search_...` and its call.
- Keep `hash_embed`, `cosine`, `chunk_body`, `MAX_CHUNK_CHARS`, and the new async `search`.
- Remove the now-unused `use crate::core::store::OkfStore;` if `build_index` was its only user.

- [ ] **Step 3: Update `state.rs`**

```rust
use crate::core::config::ConfigStore;
use crate::core::index_store::{self, PersistedIndex};
use crate::core::links::{build_link_graph, LinkGraph};
use crate::core::settings::Settings;
use crate::core::store::OkfStore;
use std::path::Path;
use std::sync::Mutex;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub index: Mutex<PersistedIndex>,
    /// Where `index.json` lives (app-config dir). Needed to persist after rebuilds.
    pub index_path: std::path::PathBuf,
    pub links: Mutex<LinkGraph>,
    pub config: ConfigStore,
}

/// Load the persisted retrieval index from disk. NEVER embeds and NEVER hits the
/// network — a missing/corrupt file yields an empty index so the app always launches.
pub fn initial_index(index_path: &Path) -> PersistedIndex {
    index_store::load(index_path)
}

/// Build the link graph from a wiki path, returning empty for an unset path
/// or any read failure (mirrors `initial_index`'s fail-soft behavior).
pub fn initial_links(wiki_path: &str) -> LinkGraph {
    if wiki_path.is_empty() {
        return LinkGraph::default();
    }
    build_link_graph(&OkfStore::new(wiki_path)).unwrap_or_default()
}
```

Rewrite the two `initial_index` tests:

```rust
#[test]
fn missing_index_file_yields_empty_index() {
    let dir = tmp();
    let idx = initial_index(&crate::core::index_store::index_path(&dir));
    assert!(idx.pages.is_empty());
}

#[test]
fn loads_persisted_index_from_disk() {
    use crate::core::index_store::{save, index_path, PersistedIndex, PageEntry, Chunk};
    let dir = tmp();
    let path = index_path(&dir);
    let mut idx = PersistedIndex { embedder_id: "hash-fnv-256".into(), ..Default::default() };
    idx.pages.insert("concepts/x.md".into(), PageEntry {
        content_hash: 1,
        chunks: vec![Chunk { path: "concepts/x.md".into(), chunk_id: 0, text: "x".into(), vector: vec![0.0] }],
    });
    save(&path, &idx).unwrap();
    assert_eq!(initial_index(&path).pages.len(), 1);
}
```

> Keep the existing link-graph tests unchanged. The `tmp()` helper already exists.

- [ ] **Step 4: Update `lib.rs` setup**

In the setup closure, after building `config` and loading `settings`, resolve the index path from the SAME config dir and load it. Replace `initial_index(&settings.wiki_path)` usage:

```rust
let dir = app
    .path()
    .app_config_dir()
    .expect("resolving app config dir");
let config = ConfigStore::new(dir.clone(), Box::new(KeyringSecretStore::new()));
let settings = config.load();
let index_path = crate::core::index_store::index_path(&dir);
let index = initial_index(&index_path);
let links = initial_links(&settings.wiki_path);
app.manage(AppState {
    settings: Mutex::new(settings),
    index: Mutex::new(index),
    index_path,
    links: Mutex::new(links),
    config,
});
```

> Match the existing variable/argument names in `lib.rs` (e.g. the exact `KeyringSecretStore::new()` constructor and how `app` is named in the `setup` closure). Add `crate::commands::reindex` to the `invoke_handler!`/`generate_handler!` list (registered now even though Task 9 defines it — define a placeholder if needed, but Task 9 is committed before any UI calls it; if you implement Tasks in order, add `reindex` to the handler in Task 9 instead to keep this task compiling). **Decision:** add `reindex` to the handler in Task 9, not here.

- [ ] **Step 5: Update `commands.rs`**

New imports at the top:

```rust
use crate::core::{
    ask::ask,
    digest::digest,
    embed::make_embedder,
    fetch::fetch_clean,
    index_store::{self, rebuild_index},
    links::{build_link_graph, segment_body, Segment},
    retrieval::search,
    settings::{make_provider, Settings},
    store::OkfStore,
};
```

Rewrite `set_settings` to be async and embed-aware:

```rust
/// Persist settings, then rebuild + persist the retrieval index for the new config.
///
/// `config.save()` runs first so a persistence failure leaves in-memory state untouched.
/// The index is rebuilt with the newly-selected embedder (a changed embedder id forces a
/// full re-embed inside `rebuild_index`). No `MutexGuard` is held across the `.await`.
#[tauri::command]
pub async fn set_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    state.config.save(&settings).map_err(|e| e.to_string())?;
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let store = OkfStore::new(settings.wiki_path.clone());
    let links = crate::state::initial_links(&settings.wiki_path);

    let prev = state.index.lock().unwrap().clone();
    let next = rebuild_index(&store, embedder.as_ref(), &prev)
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;

    *state.index.lock().unwrap() = next;
    *state.links.lock().unwrap() = links;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}
```

In `submit_source`, replace the synchronous index rebuild (old line 162) with the embed-aware path. After `s.write_page` / `s.append_log`:

```rust
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let prev = state.index.lock().unwrap().clone();
    let next = rebuild_index(&s, embedder.as_ref(), &prev)
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = next;
    *state.links.lock().unwrap() = build_link_graph(&s).map_err(|e| e.to_string())?;
```

> `settings` is already cloned at the top of `submit_source` (drop the guard before await — it already does). `prev` clones the index and drops its guard before the `.await`. Good.

Rewrite `ask_question` to embed the query and search the persisted index:

```rust
#[tauri::command]
pub async fn ask_question(
    state: State<'_, AppState>,
    question: String,
) -> Result<AnswerDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let provider = make_provider(&settings).map_err(|e| e.to_string())?;
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let index = state.index.lock().unwrap().clone();

    let hits = search(embedder.as_ref(), &question, &index, 4)
        .await
        .map_err(|e| e.to_string())?;
    let a = ask(provider.as_ref(), &question, &hits)
        .await
        .map_err(|e| e.to_string())?;
    Ok(AnswerDto {
        text: a.text,
        citations: a.citations,
    })
}
```

> `hits` borrows from the local `index` clone — both are locals, so the borrow is fine and no guard is held across `.await`.

- [ ] **Step 6: Update the integration test**

In `tests/commands_integration.rs`, change the imports and the `full_loop_digest_then_ask` test to use a real offline `HashEmbedder` + `rebuild_index` + new `search`/`ask`:

```rust
use okf_llm_wiki_lib::core::embed::HashEmbedder;
use okf_llm_wiki_lib::core::index_store::{rebuild_index, PersistedIndex};
use okf_llm_wiki_lib::core::retrieval::search;
use okf_llm_wiki_lib::core::{ask::ask, digest::digest, store::OkfStore};
```

Replace the retrieval/ask portion of `full_loop_digest_then_ask` (old lines 40–46):

```rust
    let embedder = HashEmbedder;
    let index = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    let hits = search(&embedder, "vitamin d sleep", &index, 4).await.unwrap();
    let ap = FakeProvider {
        reply: "Morning dose [concepts/vitamin-d-sleep.md]".into(),
    };
    let a = ask(&ap, "vitamin d sleep", &hits).await.unwrap();
    assert!(a.text.contains("Morning"));
    assert_eq!(a.citations, vec!["concepts/vitamin-d-sleep.md".to_string()]);
```

> Leave `backlinks_resolve_across_pages` untouched.

- [ ] **Step 7: Run the full Rust suite**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test`
Expected: PASS (all unit + integration). Then `$HOME/.cargo/bin/cargo clippy --all-targets` warning-free and `$HOME/.cargo/bin/cargo fmt`.

- [ ] **Step 8: Commit (after 🔒 security review)**

```bash
git add src-tauri/src/core/ask.rs src-tauri/src/core/retrieval.rs src-tauri/src/state.rs src-tauri/src/lib.rs src-tauri/src/commands.rs src-tauri/tests/commands_integration.rs
git commit -m "feat: switch Ask to persisted embedded chunk index

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 9: `reindex` command + register in handler

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing test**

`reindex` is a thin Tauri command; test its core logic via the building blocks in an integration test. Add to `tests/commands_integration.rs`:

```rust
#[tokio::test]
async fn reindex_from_empty_prev_embeds_all_pages() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);
    let r = digest(
        &FakeProvider { reply: r#"{"title":"Alpha","description":"d","tags":[],"body":"**TL;DR.** a."}"#.into() },
        "src", None, None, &[],
    ).await.unwrap();
    store.write_page(&r.page).unwrap();

    // reindex == rebuild against a default (empty) index
    let embedder = HashEmbedder;
    let idx = rebuild_index(&store, &embedder, &PersistedIndex::default()).await.unwrap();
    assert_eq!(idx.pages.len(), 1);
    assert_eq!(idx.embedder_id, "hash-fnv-256");
}
```

- [ ] **Step 2: Run to verify it fails / passes minimally**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test --test commands_integration reindex_from_empty_prev`
Expected: PASS already (it exercises existing building blocks) — this guards the `reindex` semantics. If it fails, fix imports.

- [ ] **Step 3: Add the `reindex` command**

In `commands.rs`:

```rust
/// Force a full rebuild of the retrieval index from scratch (ignores any reuse).
/// Passing a default `PersistedIndex` (empty `embedder_id`) guarantees an id mismatch,
/// so every page is re-embedded with the currently-selected embedder.
#[tauri::command]
pub async fn reindex(state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let store = OkfStore::new(settings.wiki_path.clone());

    let next = rebuild_index(&store, embedder.as_ref(), &index_store::PersistedIndex::default())
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = next;
    Ok(())
}
```

- [ ] **Step 4: Register in `lib.rs`**

Add `commands::reindex` to the command list in the `invoke_handler` (`tauri::generate_handler![ ... , reindex]`). Match the exact macro/path style already used for `submit_source`, `ask_question`, etc.

- [ ] **Step 5: Run the full suite**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test && $HOME/.cargo/bin/cargo clippy --all-targets`
Expected: PASS, warning-free. Then `$HOME/.cargo/bin/cargo fmt`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/tests/commands_integration.rs
git commit -m "feat: add reindex command for full index rebuild

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 10: Frontend — `reindex()` + Settings type + UI

**Files:**
- Modify: `src/lib/api.ts`
- Modify/Create: `src/lib/api.test.ts` (use the existing frontend test file if one exists)
- Modify: the current settings UI component (find it — likely `src/lib/components/Settings.svelte` or referenced from `src/routes/+page.svelte`)

- [ ] **Step 1: Write the failing frontend test**

In the frontend test file (vitest), mock `@tauri-apps/api/core`'s `invoke` and assert the new wrapper + Settings shape:

```ts
import { describe, it, expect, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
import { invoke } from "@tauri-apps/api/core";
import { reindex } from "./api";

describe("reindex", () => {
  it("invokes the reindex command", async () => {
    await reindex();
    expect(invoke).toHaveBeenCalledWith("reindex");
  });
});
```

> Match the existing test's mocking style for `invoke` if the project already mocks it differently.

- [ ] **Step 2: Run to verify it fails**

Run: `npm run test`
Expected: FAIL — `reindex` not exported from `./api`.

- [ ] **Step 3: Implement in `api.ts`**

Add to the `Settings` type the three fields (match the existing field-casing — Rust serde uses snake_case, so the IPC payload is snake_case):

```ts
export type Settings = {
  // ...existing fields...
  embed_provider: string;
  embed_model: string;
  ollama_url: string;
};
```

Add the wrapper:

```ts
export async function reindex(): Promise<void> {
  await invoke("reindex");
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `npm run test`
Expected: PASS.

- [ ] **Step 5: Update the settings UI (neo-brutalist)**

In the settings component, add controls bound to the three new `Settings` fields and a Reindex button. Keep the existing neo-brutalist styling (thick borders, hard shadows, no gradients). Example (Svelte 5 runes — adapt to the component's existing state pattern):

```svelte
<label class="field">
  <span>Embedding provider</span>
  <select bind:value={settings.embed_provider}>
    <option value="hash">hash (offline)</option>
    <option value="ollama">ollama</option>
  </select>
</label>

{#if settings.embed_provider === "ollama"}
  <label class="field">
    <span>Ollama URL</span>
    <input type="text" bind:value={settings.ollama_url} placeholder="http://localhost:11434" />
  </label>
  <label class="field">
    <span>Embedding model</span>
    <input type="text" bind:value={settings.embed_model} placeholder="nomic-embed-text" />
  </label>
{/if}

<button class="brutal-btn" onclick={runReindex} disabled={reindexing}>
  {reindexing ? "Reindexing…" : "Reindex wiki"}
</button>
{#if reindexError}<p class="error">{reindexError}</p>{/if}
```

Wire the handler (import `reindex` from `$lib/api`):

```svelte
<script lang="ts">
  import { reindex } from "$lib/api";
  let reindexing = $state(false);
  let reindexError = $state<string | null>(null);
  async function runReindex() {
    reindexing = true;
    reindexError = null;
    try {
      await reindex();
    } catch (e) {
      reindexError = String(e);
    } finally {
      reindexing = false;
    }
  }
</script>
```

> Reuse the component's existing `settings` state object and save flow — do NOT introduce a second source of truth. Match existing class names / styling conventions.

- [ ] **Step 6: Type-check + test**

Run: `npm run check && npm run test`
Expected: PASS / no svelte-check errors.

- [ ] **Step 7: Commit**

```bash
git add src/lib/api.ts src/lib/api.test.ts src/lib/components/Settings.svelte
git commit -m "feat: add embed settings + reindex button to UI

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

> Adjust the `git add` paths to the actual settings component path.

---

### Task 11: Remove dead `LlmProvider::embed`

**🔒 SECURITY REVIEW before commit** (touches `provider/`).

Embedding no longer goes through `LlmProvider`. Remove the now-unused trait method and its impls.

**Files:**
- Modify: `src-tauri/src/core/provider/mod.rs`
- Modify: `src-tauri/src/core/provider/claude.rs`
- Modify: `src-tauri/src/core/provider/fake.rs`

- [ ] **Step 1: Remove the method**

In `provider/mod.rs`, delete `async fn embed(&self, text: &str) -> Result<Vec<f32>>;` from the `LlmProvider` trait.

- [ ] **Step 2: Remove the impls**

- In `claude.rs`, delete the `embed` impl (the `Ok(crate::core::retrieval::hash_embed(text))` stub). Remove any now-unused imports.
- In `fake.rs`, delete the `embed` impl and the test assertion `assert_eq!(p.embed("abc").await.unwrap().len(), 26);` (rename/trim the test `fake_completes_and_embeds` → `fake_completes` and keep the `complete` assertion).

- [ ] **Step 3: Run the full suite**

Run: `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test && $HOME/.cargo/bin/cargo clippy --all-targets`
Expected: PASS, warning-free (no "method never used" warnings remain). Then `$HOME/.cargo/bin/cargo fmt`.

- [ ] **Step 4: Commit (after 🔒 security review)**

```bash
git add src-tauri/src/core/provider/
git commit -m "refactor: remove unused LlmProvider::embed

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Final verification (after all tasks)

- [ ] `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo test` — all green.
- [ ] `cd /Users/sonanh/Documents/AIBLES/okf-llm-wiki/src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets` — warning-free.
- [ ] `npm run check && npm run test` — green.
- [ ] Dispatch the final code-reviewer over the whole branch, then use `superpowers:finishing-a-development-branch`.

---

## Self-Review

**Spec coverage:**
- Embedder seam (`make_embedder`, hash/ollama) → Tasks 3, 4. ✅
- `HashEmbedder` offline default → Task 3. ✅
- `OllamaEmbedder` outbound HTTP, non-2xx → Err → Task 4. ✅
- `chunk_body` paragraph-aware packing (800, never split, oversize own chunk, empty→none) → Task 2. ✅
- `index_store` types + `content_hash` (NUL sep) + `load`/`save`/`index_path`/`flatten` → Task 5. ✅
- `rebuild_index` incremental reuse / id-mismatch full rebuild / empty→title chunk / drop removed / store RAW text, embed title-prefixed → Task 6. ✅
- async `search` + dim-mismatch stale guard + tie-break → Task 7 (as `search_index`), renamed in Task 8. ✅
- Settings 3 fields + config persistence (with serde defaults for back-compat) → Task 1. ✅
- Remove `LlmProvider::embed` → Task 11. ✅
- `AppState.index: Mutex<PersistedIndex>` + `index_path`; startup loads, never embeds/network → Tasks 8 (state) + 8 (lib). ✅
- Commands async + MutexGuard-safe (`submit_source`/`ask_question`/`set_settings`) + `reindex` → Tasks 8, 9. ✅
- `ask` takes `&[&Chunk]`, context = matched chunk text, deduped citations in rank order → Task 8. ✅
- Frontend `reindex()` + Settings type + neo-brutalist UI → Task 10. ✅
- No new dependencies → confirmed (reqwest/serde/async-trait/anyhow all already in `Cargo.toml`). ✅

**Type consistency:** `Chunk`/`PageEntry`/`PersistedIndex` field names are used identically in Tasks 5–10. `make_embedder` returns `Arc<dyn Embedder>`; `.as_ref()` passes `&dyn Embedder` to `rebuild_index`/`search` (which take `&dyn Embedder`). `Embedder::id()` returns `String` everywhere. `content_hash(title, body)` order is consistent. ✅

**Notable deviations from the spec (intentional refinements):**
1. `chunk_body(body)` takes only the body (not `(title, body)`) — the title prefix is applied in `rebuild_index` where the empty-body→title fallback also lives. Avoids an unused parameter; behavior identical to the spec.
2. The index lives in the **app-config dir** (same dir as `settings.json`, via `app_config_dir()`), satisfying the spec's "app-data dir, next to ConfigStore." A dedicated `index_path` field on `AppState` carries it (rather than exposing `ConfigStore.dir`).
3. New async `search` is introduced under the temporary name `search_index` in Task 7 to coexist with the old sync `search`, then renamed to `search` in Task 8 once the old one is deleted (Rust forbids two same-named fns in one module).
4. `config.rs::PersistedSettings` gains the 3 embed fields (beyond the spec's explicit file list) so the embed config actually persists across restarts.
5. **Known minor:** in `set_settings`, if the selected embedder is unreachable (e.g. Ollama down), `config.save` has already persisted the new settings to disk before `rebuild_index` fails and the command returns `Err`; in-memory `settings`/`index` stay on the old values until a successful `set_settings`/`reindex` or restart. Acceptable for a single-user local app; `reindex` provides recovery.
