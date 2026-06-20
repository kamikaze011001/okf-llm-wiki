# M3 — Make it a Real Wiki — Design Spec

> Milestone 3 of the roadmap (`docs/roadmap.md`). Turns the flat list of OKF pages into an
> interlinked wiki: `[[concept]]` links render as in-app navigation, each page shows its
> backlinks, and new pages auto-link to existing concepts at digest time.

**Date:** 2026-06-20 · **Status:** Approved (design) · **Branch:** `feat/m3-real-wiki`

---

## Goal

After M3: writing or generating `[[Concept Title]]` in an OKF page body produces a clickable
link to that concept; every page shows what links to it ("Linked from"); and digesting a new
source automatically links it to existing concepts it mentions. The concept-graph
visualization is **deferred** to a later milestone.

## Problem (grounded in current code)

| Gap | Location | Symptom |
|---|---|---|
| Pages are not interlinked | OKF bodies are plain Markdown; no link concept exists | The wiki is a flat list of notes, not a navigable graph |
| Bodies render as raw text | `Browse.svelte:16` (`white-space:pre-wrap`, no markdown/link parsing) | `[[...]]` would show as literal text, not navigation |
| No backlinks | (not implemented) | A page can't show what references it |
| No auto-linking | `digest.rs` | New pages never connect to existing concepts |

## Decisions

- **Link syntax:** `[[Concept Title]]` — standard double-bracket wiki link; the bracketed text is the visible label. Stays human-readable in the raw `.md` (OKF portability preserved).
- **Resolution via `slugify`:** a page's *slug* is its filename stem (`concepts/<slug>.md`). `[[X]]` resolves by comparing `slugify(X)` against existing page slugs — identical to how pages are named at digest time, so there is exactly one resolution rule.
- **Link logic lives in Rust core** (approach A): a new `core/links.rs` owns parsing, resolution, the link graph, and validation. The frontend renders backend-provided segments and backlinks; it never re-implements parsing or `slugify` (no cross-language drift). Core stays Tauri-free (ADR-0001).
- **Auto-linking = LLM-suggested + deterministic validation:** the digest prompt carries an allow-list of existing concept titles; a pure post-pass drops any `[[link]]` whose target doesn't resolve, guaranteeing no dangling auto-links.
- **Red links** (unresolved `[[X]]`) render with distinct styling; clicking is inert in M3 (the create-from-red-link flow is M4 capture/edit territory).
- **No display aliases** (`[[target|alias]]`) — YAGNI for M3.

## Architecture

Core stays Tauri-free. The link graph has the same lifecycle as the retrieval index: built at
startup and rebuilt wherever the index is rebuilt.

```
core/links.rs (new, Tauri-free)
  extract_links(body)      -> Vec<Link { text, target_slug }>
  segment_body(body)       -> Vec<Segment>            // Text | Link, ordered, for rendering
  validate_links(body, &known_slugs) -> String        // unwrap unresolved [[X]] to plain text
  build_link_graph(store)  -> LinkGraph
  LinkGraph { path_for(slug), exists(slug), backlinks(path) -> Vec<BacklinkRef> }

state.rs    AppState gains `links: Mutex<LinkGraph>`  (built in setup, rebuilt on writes)
commands.rs new `get_page_view(path) -> PageViewDto`
digest.rs   digest(..) gains `existing: &[ConceptRef]`; validates generated body
```

### Components

**`core/links.rs` (new)**

Types:
- `pub struct Link { pub text: String, pub target_slug: String }`
- `pub enum Segment { Text(String), Link { text: String, target_slug: String } }`
- `pub struct BacklinkRef { pub path: String, pub title: String }`
- `pub struct ConceptRef { pub slug: String, pub title: String }`
- `pub struct LinkGraph { slug_to_path: HashMap<String, String>, slug_to_title: HashMap<String, String>, backlinks: HashMap<String, Vec<String>> }`

Functions:
- `extract_links(body: &str) -> Vec<Link>` — scan for `[[ … ]]`; inner text trimmed; `target_slug = slugify(text)`. Empty (`[[]]`) and unbalanced (`[[` with no closing `]]`) brackets are ignored and treated as plain text. Never panics (byte/char-safe scan).
- `segment_body(body: &str) -> Vec<Segment>` — split the body into ordered `Text` and `Link` runs, preserving all non-link text exactly (so rendering can reproduce the body). A `[[X]]` with empty/whitespace inner text stays `Text`.
- `validate_links(body: &str, known: &HashSet<String>) -> String` — for each `[[X]]`, if `slugify(X)` ∉ `known`, replace `[[X]]` with `X` (unwrap); otherwise leave intact. Case-insensitive by construction (`slugify` lowercases).
- `build_link_graph(store: &OkfStore) -> Result<LinkGraph>` — for each page: record `slug → path` and `slug → title` (slug = filename stem of the path; title from frontmatter, falling back to the slug). Then for each page's body links, for each resolved `target_slug` that exists, push the source page's path into `backlinks[target_slug]`. Self-references (source slug == target slug) are excluded.
- `LinkGraph::path_for(&self, slug: &str) -> Option<&str>`, `exists(&self, slug: &str) -> bool`, `backlinks(&self, path: &str) -> Vec<BacklinkRef>` (resolves the source paths to `{path, title}`, sorted by path for determinism).

Helper: `slug_of(path: &str) -> &str` — filename stem (strip `concepts/` dir and `.md`).

**`core/digest.rs` (modify)**
- `digest(provider, source_text, resource, note, existing: &[ConceptRef]) -> Result<DigestResult>`.
- System prompt appends, when `existing` is non-empty: a list of existing concept titles and the instruction to wrap exact mentions in `[[ ]]`, using only titles from the list.
- After parsing `DigestJson`, compute `known = existing.iter().map(|c| c.slug)` **plus the new page's own slug**, then `body = validate_links(&parsed.body, &known)` before constructing the `Page`. (The page may legitimately self-reference, and may link to any existing concept.)

**`state.rs` (modify)**
- `AppState` gains `links: Mutex<LinkGraph>`.
- `initial_links(wiki_path: &str) -> LinkGraph` mirroring `initial_index`: empty path → empty graph; else `build_link_graph(&OkfStore::new(wiki_path)).unwrap_or_default()` (`LinkGraph: Default`).

**`lib.rs` (modify)**
- In `setup`, build the link graph alongside the index and `manage` it in `AppState`.

**`commands.rs` (modify)**
- `submit_source`: gather `existing: Vec<ConceptRef>` from the store's pages, pass to `digest`; after writing, rebuild **both** index and link graph.
- `set_settings`: rebuild the link graph from the new `wiki_path` alongside the index (same `MutexGuard`-discipline as today — no guard held across blocking IO).
- New `get_page_view(state, path) -> Result<PageViewDto, String>`: read the page, `segment_body` its body, resolve each `Link` segment against the graph (`exists` + `path_for`), and attach `backlinks`.

DTOs (in `commands.rs`):
- `PageViewDto { path, title, tags: Vec<String>, note: Option<String>, resource: Option<String>, segments: Vec<SegmentDto>, backlinks: Vec<RefDto> }`
- `SegmentDto { kind: String /* "text" | "link" */, text: String, target_path: Option<String>, exists: bool }`
- `RefDto { path: String, title: String }`

**`src/lib/api.ts` (modify)**
- Add `getPageView(path: string): Promise<PageView>` and the `PageView` / `Segment` / `Ref` types, following the existing throw-on-error invoke pattern.

**`src/lib/components/Browse.svelte` (modify)**
- On selected page change, call `getPageView($currentPage)`.
- Render `segments`: `text` → plain span (keep `white-space:pre-wrap`); `link` with `exists` → clickable neo-brutalist link that sets `$currentPage = target_path`; `link` without `exists` → red-link styling, inert.
- Add a **"Linked from"** section listing `backlinks` (click → set `$currentPage`). Hidden when empty.

## Data flow

```
Startup:  build_index + build_link_graph → manage AppState { settings, index, links, config }
Capture:  submit_source → gather existing ConceptRefs → digest (prompt allow-list)
            → validate_links(body, known) → write page → rebuild index + link graph
Browse:   select page → get_page_view → segment_body + resolve + backlinks → render
Settings: set_settings → rebuild index + link graph from new wiki_path
```

## Error handling

- Unbalanced `[[`, empty `[[]]`, or punctuation-only inner text → treated as plain text; no panic.
- Empty wiki / first page (empty allow-list) → prompt states no existing concepts; no auto-links added.
- Page with no backlinks → empty "Linked from" section (hidden).
- Self-reference → excluded from backlinks.
- `get_page_view` on a missing path → `Err(String)` surfaced to the UI (consistent with other commands).
- Corrupt/unreadable page during `build_link_graph` → propagate the `Result` error (same as `build_index`); startup degrades to an empty graph via `unwrap_or_default`, matching `initial_index`.

## Testing

- `core/links.rs`:
  - `extract_links`: single, multiple, none, unbalanced `[[`, empty `[[]]`, punctuation/case (`[[Vitamin D & Sleep]]` → slug `vitamin-d-sleep`).
  - `segment_body`: leading/trailing text, adjacent links, body round-trips (concatenating segment text reproduces the body minus the `[[ ]]` markers).
  - `validate_links`: drops unknown, keeps known, case-insensitive, leaves non-link text untouched.
  - `build_link_graph` + `backlinks`: B links A → `backlinks(A)` includes B; link to a non-existent slug → no backlink + `exists` false; self-link excluded.
- `core/digest.rs`: prompt includes existing concept titles when provided; FakeProvider returns a body with one valid + one invalid `[[link]]` → the invalid one is unwrapped, the valid one survives; empty `existing` → no allow-list text, body unchanged.
- Integration (`tests/commands_integration.rs`): capture page A, then page B whose body references A's title → `get_page_view(A)` backlinks include B, and B's matching segment is a resolved link to A.
- Frontend (`api.test.ts`): `getPageView` resolves to the expected shape; errors throw.
- Regression: existing Rust + frontend tests stay green; `cargo clippy --all-targets` clean; `cargo fmt`.

## Security review

M3 does **not** touch the listed sensitive areas (`settings.rs`, `state.rs` key handling, `store.rs` writes, `provider/`). `state.rs` gains a non-secret `links` field only; `store.rs` is read via existing APIs (no new write path). A security review is therefore **not required** by the workflow gate, but the digest prompt change (now includes existing concept titles) should be sanity-checked to confirm no secret or unexpected data enters the prompt — titles are non-sensitive OKF content.

## Dependencies added

None. `std::collections::{HashMap, HashSet}` and the existing `slugify` cover M3.

## Out of scope

- Concept-graph / index **visualization** (its own follow-up).
- Manual **edit/delete** of pages and the **red-link → create** flow (M4).
- Full-Markdown rendering of bodies (bodies still render as text apart from `[[links]]`).
- Display aliases `[[target|alias]]`.
- M2 (real embeddings, persisted index, chunking).
