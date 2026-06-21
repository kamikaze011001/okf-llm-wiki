# M4 (slice 2) — Red-link → Create Page — Design

> **Milestone:** M4 (Zero-barrier UX), second sub-project.
> **Goal:** Click an unresolved `[[link]]` in Browse to create that page on the spot, so the
> wiki grows organically and the link turns blue immediately.
> **Status:** Design approved 2026-06-20.

---

## Problem

Red-links — `[[X]]` mentions whose page does not exist yet — are dead ends. `get_page_view`
resolves each `[[link]]` against the in-memory `LinkGraph`; an unresolved one becomes
`SegmentDto { kind:"link", exists:false, target_path:None, text:<inner> }`, which
`Browse.svelte` renders as a `<span class="nb-redlink">` with `cursor:not-allowed` and no click
handler. The only way to fill that gap today is to capture an unrelated source and hope the LLM
picks the same title. This slice makes the red-link itself the create affordance.

## Scope

**In scope**
- Click a red-link in Browse → instantly create a **stub** page at the link's **exact slug**.
- Open the new page in the existing inline edit form (from the edit/delete slice) so the user
  fills it in.
- Keep the retrieval index and link graph correct, and record the creation in `log.md`.

**Out of scope (deferred)**
- **Any LLM/digest of the stub.** Creating a stub makes no API call and needs no key; the user
  writes the body via the edit flow already shipped. "Digest a source into this fixed slug" is a
  separate, larger slice.
- **Forcing a slug onto a digested capture.** Not needed: the stub owns the slug directly.
- **Multi-step collision UI.** A collision is a plain error string (see Error handling).

## Why the stub resolves the link

The stub's path is `concepts/{slugify(title)}.md`, and the title passed in is the red-link's
display text (`seg.text`). `slugify(seg.text)` is exactly the `target_slug` the red-link was
looking for. After the post-write `build_link_graph`, the source page's `[[X]]` resolves to the
new path and renders blue on the next view. This sidesteps the digest-title→slug mismatch problem
entirely (a digested capture lets the LLM choose the title, so its slug may differ from the link).

## Decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Create behavior | Stub at the exact slug + open in the existing edit form (no LLM, no key) |
| Collision (slug already on disk) | **Hard error**, never overwrite; nothing is written |
| Empty slug (e.g. blank title) | Error; nothing written (defensive — red-links always slugify non-empty) |
| Stub body | Placeholder `"_Stub — fill this in._"` (not empty: makes the stub obvious; empty would index no chunks) |
| `log.md` | Append `- created <title>` (activity trail, like `edited`/`deleted`) |

## Architecture

Follows the existing `update_page`/`delete_page` mutation pattern exactly: **write the file →
rebuild + persist the retrieval index → rebuild the link graph → append to `log.md`**, via the
shared `refresh_index_and_links` helper. One pure builder, one small shared timestamp module, one
Tauri command, and a clickable red-link in `Browse.svelte`.

Invariants preserved: `core/` stays Tauri-free; atomic writes via the existing `store.write_page`;
no `MutexGuard` held across an `.await` in `commands.rs` (clone/drop before await); neo-brutalist
UI; OKF files stay portable Markdown + YAML.

**Path safety:** `slugify` emits only `[a-z0-9-]`, so a stub slug can never contain `/` or `..` —
no path-traversal surface, and `store.rs` is not modified.

### Backend

**`src-tauri/src/core/clock.rs`** *(new, tiny)*
- Move `now_iso()` out of `digest.rs` into `core::clock::now_iso` (RFC-3339 UTC via the `time`
  crate) so `digest` and `create` share one timestamp source. `digest.rs` calls
  `crate::core::clock::now_iso()`; its existing `now_iso_is_rfc3339` test moves with it (or is
  re-pointed). Register `pub mod clock;` in `core/mod.rs`.

**`src-tauri/src/core/create.rs`** *(new)*
- Pure `pub fn new_stub_page(title: &str) -> Page`:
  ```
  Page {
    path: format!("concepts/{}.md", slugify(title)),
    frontmatter: Frontmatter {
      type_: "Concept", title: Some(title.to_string()),
      description: None, tags: vec![], resource: None,
      timestamp: Some(crate::core::clock::now_iso()), note: None,
      extra: BTreeMap::new(),
    },
    body: "_Stub — fill this in._".into(),
  }
  ```
- Register `pub mod create;` in `core/mod.rs`.

**`src-tauri/src/commands.rs`** — one new async command:
- `create_page(state, title: String) -> Result<PageDto, String>`
  - Clone settings out of the mutex (drop the guard before any `.await`).
  - Guard: `let slug = slugify(&title);` → if `slug.is_empty()` return `Err("cannot create a page with an empty title")`.
  - Guard: build `path = concepts/{slug}.md`; if `store.read_page(&path).is_ok()` return
    `Err(format!("a page for \"{title}\" already exists"))` — never overwrite.
  - `let page = new_stub_page(&title);` → `store.write_page(&page)` →
    `store.append_log(&format!("created {title}"))` → `refresh_index_and_links(&state, &store, &settings).await?`.
  - Return the `PageDto` for `page` (same field mapping as `submit_source`).

**`src-tauri/src/lib.rs`**
- Register `commands::create_page` in `generate_handler!`.

### Frontend

**`src/lib/api.ts`**
- `export const createPage = (title: string) => invoke<PageDto>("create_page", { title });`

**`src/lib/components/Browse.svelte`**
- New local state: `creating = false`, `createError = ""`, `pendingEdit = false`.
- Red-link branch in the article becomes clickable (keep the `nb-redlink` look but make it an
  actionable element — an `<a href="#/" on:click|preventDefault>` styled as the red-link, title
  "Create this page"): on click → `createFromLink(seg.text)`.
- `createFromLink(title)`: set `creating`, clear `createError`; `try` `const p = await createPage(title)`
  → `pendingEdit = true` → `currentPage.set(p.path)` (triggers `loadFor`); `catch` set
  `createError = String(e)`; `finally` `creating = false`.
- `loadFor` consumes `pendingEdit`: after `view = await getPageView(path)`, if `pendingEdit` then
  `pendingEdit = false; startEdit();` else stay in view mode. Reset `createError` in `loadFor`
  alongside the other per-load resets.
- `createError` rendered as a `nb-card` (red) in view mode, like `deleteError`.

## Data flow

```
click red [[Vitamin D]] → createFromLink("Vitamin D") → create_page("Vitamin D")
  slug = "vitamin-d"  (non-empty, not already on disk)
  → write_page(concepts/vitamin-d.md, stub) → append_log("created Vitamin D")
  → rebuild_index + save → build_link_graph → AppState
  → return PageDto
  → Browse: pendingEdit=true; currentPage="concepts/vitamin-d.md"
  → loadFor reloads the view and opens edit mode
  (the original page's [[Vitamin D]] resolves blue on its next view)
```

## Error handling

- **Collision** (a page already owns that slug) → `Err("a page for \"<title>\" already exists")`,
  surfaced as a red `nb-card`; nothing written. This shouldn't normally happen (a resolved link
  renders blue, not red), but guards against races and stale views.
- **Empty slug** → `Err("cannot create a page with an empty title")`; nothing written.
- **Index/link rebuild failure after the write** → `Err` string; the file is on disk while
  in-memory index/links stay behind, recoverable via `reindex` — the same accepted trade-off as
  `submit_source`/`update_page`/`delete_page` for a single-user local app.

## Testing

**Rust unit — `core/create.rs`**
- `new_stub_page` produces `path = concepts/<slug>.md`, `type_ = "Concept"`, `title = Some(input)`,
  empty tags, a non-empty RFC-3339 timestamp, and the placeholder body.
- Slug derivation matches `slugify` (e.g. `"Vitamin D & Sleep"` → `concepts/vitamin-d-sleep.md`).

**Rust integration — `tests/commands_integration.rs`**
- **Resolves a red-link:** seed Beta with body `Beta builds on [[Ghost]].`; assert
  `build_link_graph` leaves `ghost` unresolved; write `new_stub_page("Ghost")`; rebuild the graph;
  assert Beta's `[[Ghost]]` now resolves (`graph.path_for("ghost").is_some()`).
- **Collision:** writing a stub whose slug already exists is detectable (the command's guard) —
  assert `store.read_page("concepts/ghost.md").is_ok()` after the first create, the signal the
  command uses to refuse a second.
- **Log:** after a create, `log.md` contains `- created Ghost`.
  (As with the edit/delete tests, the command path is exercised via its composed core primitives,
  since constructing a Tauri `State` in a unit test is impractical.)

**Frontend — `src/lib/api.test.ts`**
- `createPage("Ghost")` invokes `"create_page"` with `{ title: "Ghost" }`.

**Gates**
- `cargo test` + `cargo clippy --all-targets` + `npm run test` + `npm run check` + `npm run build`
  green; `cargo fmt`.
- **No security review gate this slice:** `create_page` only composes existing `store` methods
  (`read_page`/`write_page`); `store.rs`, `settings.rs`, `state.rs`, and `provider/` are untouched.

## Files touched

| File | Change |
|---|---|
| `src-tauri/src/core/clock.rs` | new tiny module: `now_iso()` (moved from `digest.rs`) |
| `src-tauri/src/core/digest.rs` | use `core::clock::now_iso`; drop local copy |
| `src-tauri/src/core/create.rs` | new: `new_stub_page`; unit tests |
| `src-tauri/src/core/mod.rs` | `pub mod clock;` + `pub mod create;` |
| `src-tauri/src/commands.rs` | add `create_page` |
| `src-tauri/src/lib.rs` | register `create_page` |
| `src-tauri/tests/commands_integration.rs` | red-link-resolves / collision / log tests |
| `src/lib/api.ts` | `createPage` |
| `src/lib/components/Browse.svelte` | clickable red-link → create + open in edit |
| `src/lib/api.test.ts` | client test for `create_page` |
