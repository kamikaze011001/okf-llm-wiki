# M2 — Make Ask Genuinely Good — Design Spec

> Milestone 2 of the roadmap (`docs/roadmap.md`). Replaces the FNV hashing embedder with
> real, provider-agnostic embeddings (Ollama backend + hash fallback), persists the retrieval
> index to disk as a content-hashed incremental cache, and chunks long pages — so Ask
> retrieves relevant passages instead of whole-page bag-of-words matches.

**Date:** 2026-06-20 · **Status:** Approved (design) · **Branch:** `feat/m2-better-ask`

---

## Goal

After M2: Ask retrieves the most relevant *passages* of the wiki (not whole pages) using real
embeddings, the index survives restarts and only re-embeds pages that changed, and the
embedding backend is selectable (Ollama, or the offline hash fallback) without touching the
completion provider. The concept-graph visualization remains **deferred** (M4).

## Problem (grounded in current code)

| Gap | Location | Symptom |
|---|---|---|
| Embeddings are a hashing stub | `retrieval.rs:5` (`hash_embed`, FNV into 256 dims) | Bag-of-words matching; no semantic relevance |
| `LlmProvider::embed` is dead code | `provider/mod.rs:7`, `provider/claude.rs:54` | The async embed seam exists but nothing calls it; Claude "embeds" by returning `hash_embed` |
| No chunking | `retrieval.rs:49` (`build_index` — one vector per page) | Long pages get one diluted vector; no passage-level retrieval |
| Ask feeds a 160-char snippet | `ask.rs:19` | The LLM sees a truncated head of each page, not the matched passage |
| Index is in-memory only | `state.rs`, `commands.rs` (`build_index` on every write/startup) | Whole wiki re-embeds on every capture and every launch |

## Decisions

- **Embeddings are provider-agnostic behind a dedicated `Embedder` seam.** A new
  `core/embed/` module owns the trait and backends. The unused `LlmProvider::embed` method is
  removed; completion (`LlmProvider`) and embedding (`Embedder`) are independent — you can run
  Claude for answers and Ollama (or hash) for search.
- **Ship two backends:** `HashEmbedder` (wraps the existing `hash_embed`; offline **default**,
  zero setup) and `OllamaEmbedder` (local `POST /api/embeddings`). OpenAI and others are
  out of scope — the seam makes them a trivial later add.
- **The persisted index is the cache.** One JSON file in the OS app-data dir (next to
  `ConfigStore`), written atomically like `store.rs`. Startup just *deserializes* it — **no
  embedder, no network at launch** — so the app always starts even if Ollama is down.
- **Content-hashed incremental rebuild.** Each page records a content hash; only changed/new
  pages re-embed. The index header records the `embedder_id`; a mismatch forces a full rebuild.
- **Paragraph-aware chunking** (~800 chars, never splitting mid-paragraph), title prepended to
  each chunk's embedded text for context. Ask feeds the matched chunk texts to the LLM.
- **No new dependencies, no change to the OKF on-disk format** — the index lives outside the wiki.

## Architecture

Core stays Tauri-free (ADR-0001). The single structural shift: **embedding becomes async and
fallible** (a network call), so `build_index`, `search`, and the commands that rebuild the
index become async. Startup and disk-load stay synchronous because they never embed.

```
core/embed/ (new, Tauri-free)
  mod.rs    Embedder trait { embed(text) -> Vec<f32>, id() -> String }; make_embedder(&Settings)
  hash.rs   HashEmbedder   — wraps hash_embed; id "hash-fnv-256"; offline DEFAULT
  ollama.rs OllamaEmbedder — POST {base_url}/api/embeddings; id "ollama:<model>"
core/retrieval.rs   (modify) chunk_body(title, body); cosine; async search over chunks
core/index_store.rs (new)    PersistedIndex (JSON, app-data dir); load (sync) / save (atomic);
                             async rebuild_index(store, embedder, prev) — incremental
state.rs   AppState.index changes Vec<IndexEntry> -> PersistedIndex (Mutex)
commands.rs submit_source / ask_question / set_settings become embed-aware; new `reindex`
settings.rs new fields embed_provider / embed_model / ollama_url; make_embedder
```

### Components

**`core/embed/mod.rs` (new)**

```rust
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Stable identity of this embedder's vector space, e.g. "hash-fnv-256" or
    /// "ollama:nomic-embed-text". Stored in the index header; a mismatch forces a rebuild.
    fn id(&self) -> String;
}

pub fn make_embedder(s: &Settings) -> Result<Arc<dyn Embedder>>;
```

- `make_embedder`: `"hash"` → `HashEmbedder`; `"ollama"` → `OllamaEmbedder { base_url: s.ollama_url, model: s.embed_model, client }`; unknown → `Err(anyhow!(...))`.

**`core/embed/hash.rs` (new)**
- `HashEmbedder` — `embed` returns `Ok(hash_embed(text))` (the existing fn stays in `retrieval.rs`); `id()` = `"hash-fnv-256"`. Infallible, offline.

**`core/embed/ollama.rs` (new)**
- `OllamaEmbedder { base_url: String, model: String, client: reqwest::Client }`.
- `embed`: `POST {base_url}/api/embeddings` with body `{"model": model, "prompt": text}`; on non-2xx → `Err(anyhow!("Ollama API error {status}: {body}"))`; parse `.embedding` as `Vec<f32>`, else `Err(anyhow!("unexpected Ollama response shape"))`. Keyless localhost — no secret in the request. Mirrors `claude.rs` error shape.
- `id()` = `format!("ollama:{}", self.model)`.

**`core/retrieval.rs` (modify)**
- Keep `hash_embed`, `cosine`. Remove the sync `search`/`build_index`/`IndexEntry` (superseded by `index_store.rs` + async search). `hash_embed` stays public (used by `HashEmbedder` and tests).
- `pub fn chunk_body(title: &str, body: &str) -> Vec<String>`:
  - Split `body` into paragraphs on blank lines (a Markdown heading line begins a new paragraph).
  - Greedily pack paragraphs into chunks up to `MAX_CHUNK_CHARS` (800), never splitting a paragraph; a single paragraph longer than the cap becomes its own chunk.
  - Returns the **raw** chunk texts (no title prefix). Empty/whitespace body → `vec![]`; callers treat an empty result as a single title-only chunk (see `rebuild_index`).
- `pub async fn search(embedder: &dyn Embedder, query: &str, index: &PersistedIndex, k: usize) -> Result<Vec<&Chunk>>`: embed the query once, cosine against every chunk vector, sort desc, take `k`. Ties broken by `(path, chunk_id)` for determinism.

**`core/index_store.rs` (new)**

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct Chunk { pub path: String, pub chunk_id: usize, pub text: String, pub vector: Vec<f32> }

#[derive(Clone, Serialize, Deserialize)]
pub struct PageEntry { pub content_hash: u64, pub chunks: Vec<Chunk> }

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PersistedIndex {
    pub embedder_id: String,
    pub pages: BTreeMap<String, PageEntry>,  // page path -> chunks
}
```

- `content_hash(title, body) -> u64`: FNV-1a of `title + "\0" + body` (no new dep).
- `pub fn load(path: &Path) -> PersistedIndex`: deserialize JSON; any error (missing/corrupt) → `PersistedIndex::default()`. Sync, no embedder.
- `pub fn save(path: &Path, idx: &PersistedIndex) -> Result<()>`: serialize and write atomically (temp file + rename, same discipline as `store.rs`).
- `pub fn index_path(app_data_dir: &Path) -> PathBuf`: `app_data_dir.join("index.json")`.
- `pub async fn rebuild_index(store: &OkfStore, embedder: &dyn Embedder, prev: &PersistedIndex) -> Result<PersistedIndex>`:
  - `reuse = prev.embedder_id == embedder.id()`.
  - For each `path` in `store.list_pages()?`: read page; `h = content_hash(title, body)`. If `reuse` and `prev.pages[path].content_hash == h` → reuse `prev.pages[path].chunks`. Else: `let chunks = chunk_body(title, body); if chunks.is_empty() { chunks = vec![title] }`; embed `format!("{title}\n\n{chunk}")` for each, store the **raw** chunk as `text`.
  - Pages not in the store are dropped. Returns `PersistedIndex { embedder_id: embedder.id(), pages }`.
- `pub fn flatten(idx: &PersistedIndex) -> Vec<&Chunk>` helper for search/iteration (deterministic order via the `BTreeMap`).

**`core/settings.rs` (modify)** — non-secret fields only; API key stays in the keychain.

```rust
pub embed_provider: String,  // "hash" | "ollama"            (default "hash")
pub embed_model: String,     // e.g. "nomic-embed-text"      (default "nomic-embed-text")
pub ollama_url: String,      // default "http://localhost:11434"
```
- `Default` sets the three above. `Debug` redaction unchanged (still only `api_key` redacted).
- `make_embedder` lives here or in `embed/mod.rs` (re-exported); it reads these three fields.

**`core/provider/mod.rs` + `claude.rs` + `fake.rs` (modify)**
- Remove `embed` from the `LlmProvider` trait and its impls (Claude's hashing stub, Fake's bag-of-chars). Completion-only trait.
- Add a `FakeEmbedder` (in `embed/` test support or `fake.rs`) for tests: deterministic vector + a call counter to assert "unchanged page → 0 embed calls".

**`state.rs` (modify)**
- `AppState.index: Mutex<PersistedIndex>` (was `Mutex<Vec<IndexEntry>>`).
- `initial_index(app_data_dir) -> PersistedIndex` = `index_store::load(&index_store::index_path(app_data_dir))`. Sync, offline, never embeds. (Note: now keyed by app-data dir, not `wiki_path`.)

**`lib.rs` (modify)**
- In `setup`, resolve the app-data dir (via the Tauri path API, already used by `ConfigStore`), load the persisted index, `manage` it in `AppState`. No embedder at startup.

**`commands.rs` (modify)**
- `submit_source` (async): after `write_page`, `let prev = state.index.lock().clone()` (drop guard); `let next = rebuild_index(&store, embedder.as_ref(), &prev).await?` (incremental — embeds only the new page; **no lock held across await**); `index_store::save(...)?`; `*state.index.lock() = next`.
- `ask_question` (async): clone settings (drop guard) → `make_embedder` → `let idx = state.index.lock().clone()` (drop guard) → `search(embedder.as_ref(), &q, &idx, K).await?` → `ask(provider, &q, &hits)`. `K = 4` (chunks).
- `set_settings` (**now async**): persist settings JSON first (unchanged ordering); then `make_embedder` from the new settings, `rebuild_index(&store, embedder, &prev).await?` (mismatched `embedder_id` ⇒ full rebuild), save, swap. No guard across await.
- New `reindex` (async): force a full rebuild with the current embedder by calling `rebuild_index(&store, embedder, &PersistedIndex::default())` — the default's empty `embedder_id` guarantees a mismatch, so every page re-embeds. Save + swap. Returns `Ok(())` / `Err(String)`.

**`ask.rs` (modify)**
- `ask(provider, question, hits: &[&Chunk]) -> Result<Answer>`: context = each hit's `text` (the matched chunk, not a snippet) joined as `[{path}]\n{text}`; citations = hit `path`s **deduped, preserving rank order**. System prompt unchanged.

**`src/lib/api.ts` (modify)**
- Add `reindex(): Promise<void>` (`invoke("reindex")`). Extend the `Settings` type with `embed_provider`, `embed_model`, `ollama_url`.

**Settings component (modify)**
- Add inputs for embedding provider (hash | ollama), embed model, and Ollama URL (shown when provider = ollama), plus a **Reindex** button calling `reindex()`. Neo-brutalist styling consistent with existing controls.

## Data flow

```
Startup:   load index.json (sync, offline) -> manage AppState { settings, index, links, config }
Capture:   submit_source -> write page -> rebuild_index (embed only changed page) -> save -> swap
Ask:       ask_question -> make_embedder -> embed query -> cosine over chunks -> top-k chunk texts -> LLM
Settings:  set_settings -> persist JSON -> rebuild_index in (possibly new) space -> save -> swap
Reindex:   reindex -> full rebuild with current embedder -> save -> swap
```

## Error handling

- Ollama unreachable / non-2xx → `embed` returns `Err`; surfaces to the UI on capture/ask/reindex (consistent with other commands). Startup is unaffected (no embed at launch).
- Corrupt/missing `index.json` → `load` returns empty; app launches; next capture/reindex rebuilds.
- Embedder changed (`embedder_id` mismatch) → full rebuild on the next `rebuild_index`. Until then, on-disk vectors and a fresh query embedded in a *different* space would mismatch — guard: if a chunk vector length ≠ the query vector length, skip it in `cosine`/`search` (no panic, just not matched), so a stale index degrades to "no hits" rather than crashing.
- Empty wiki → empty index; Ask says context is insufficient (existing behavior).
- `MutexGuard` never held across `.await` (clone/drop before embedding) — the project's #1 concurrency rule.

## Testing

- `chunk_body`: paragraph packing respects the ~800 cap; never splits a paragraph; oversized lone paragraph → its own chunk; empty body → `vec![]` (caller makes it title-only); headings start new paragraphs.
- `embed`: `HashEmbedder.id() == "hash-fnv-256"` and equals `hash_embed`; `OllamaEmbedder` parses a canned `{"embedding":[...]}` and maps non-2xx → `Err` (no live server needed — test the parse/error paths); `FakeEmbedder` covers the async seam.
- `index_store`: `load` of missing/corrupt file → default; `save`→`load` round-trips; atomic write leaves no partial file on simulated failure; `content_hash` stable & differs on changed body.
- `rebuild_index`: unchanged page → `FakeEmbedder` call-count 0 (reuse); changed body → re-embed; deleted page drops; `embedder_id` mismatch → full rebuild (call-count = total chunks); empty body → one title-only chunk.
- `search`: ranks the relevant chunk first; query embedded in the same space; mismatched-dim vectors skipped without panic.
- `ask`: feeds chunk `text` (not snippet); citations deduped & rank-ordered.
- Integration (`tests/commands_integration.rs`): capture page A then page B → persisted index has both with content hashes; re-capturing A unchanged re-embeds nothing (call-count assertion via a fake); `ask` returns a citation to the relevant page.
- Frontend (`api.test.ts`): `reindex` invokes `"reindex"`; settings round-trip includes the three embed fields.
- Regression: existing Rust + frontend tests stay green; `cargo clippy --all-targets` clean; `cargo fmt`.

## Security review

Required by the workflow gate before committing:
- `core/settings.rs` — sensitive (API key). M2 adds only **non-secret** fields (`embed_provider`,
  `embed_model`, `ollama_url`); the keychain path is untouched. Confirm `Debug` still redacts.
- `core/embed/ollama.rs` — new **outbound-request boundary**. Confirm no API key or secret enters
  the Ollama request (keyless localhost), and nothing secret is logged or written to `index.json`.
- `core/index_store.rs` — new file write path. Confirm atomic write, and the file contains only
  OKF-derived text + vectors (no secrets).

A `security-reviewer` pass covers these before the relevant commits.

## Dependencies added

None. `reqwest`, `serde_json`, `serde`, `async-trait`, `anyhow`, `time` already present; Ollama
is one more HTTP call. FNV content hashing reuses the existing constants from `hash_embed`.

## Out of scope

- OpenAI / other embedding backends (the `Embedder` seam makes them a later one-file add).
- Approximate nearest-neighbour / vector DB — linear cosine is fine at personal-wiki scale.
- Re-ranking, hybrid keyword+vector search, query expansion.
- Embedding the `[[link]]` graph or concept-graph **visualization** (M4).
- Any change to the OKF on-disk format — the index lives outside the wiki folder.
- Persisted settings migration tooling — new fields default cleanly on load of old JSON.
