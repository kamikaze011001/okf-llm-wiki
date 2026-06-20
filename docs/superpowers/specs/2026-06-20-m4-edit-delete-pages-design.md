# M4 (slice 1) — Edit & Delete OKF Pages In-App — Design

> **Milestone:** M4 (Zero-barrier UX), first sub-project.
> **Goal:** Let the user fix and remove pages from inside the app, instead of hand-editing files on disk.
> **Status:** Design approved 2026-06-20.

---

## Problem

Today the only way to correct a bad title/tag or remove a mistaken capture is to leave the
app and edit the Markdown files by hand (`roadmap.md` gap: "No way to edit or delete a page
in-app"). Browse is read-only. This slice closes that gap for the two core operations:
**edit** and **delete**.

## Scope

**In scope**
- Edit a page's **body** (raw Markdown), **title**, **tags**, and **note** — in place in Browse.
- Delete a page, with an inline confirm step.
- Keep the retrieval index and link graph correct after both operations.
- Record both operations in `log.md`.

**Out of scope (deferred)**
- **Rename / slug change.** A title edit updates frontmatter only; the file path
  (`concepts/<slug>.md`, the page's identity) stays fixed, so `[[wiki-links]]` and backlinks
  never break. Renaming would require rewriting links across other pages and migrating index
  keys — a separate slice if ever needed.
- Re-running the LLM digest on edit. Editing saves the raw Markdown the user typed; no API
  call, no key required to edit.
- Live Markdown preview / split-pane editor. A raw `<textarea>` is sufficient and honest for
  portable Markdown.

## Decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Editable fields | body + title + tags + note; **path stays fixed** (no rename) |
| Edit UI | inline edit toggle on Browse (reuse the view, no new route) |
| `log.md` | record edits and deletes too (append-only activity trail) |
| Edit mechanism | direct raw-Markdown editing (no re-digest) |

## Architecture

Follows the existing `submit_source` mutation pattern exactly: **mutate the file → rebuild +
persist the retrieval index → rebuild the link graph → append to `log.md`**. Two focused Tauri
commands, one small `store` addition, and inline UI in `Browse.svelte`.

Invariants preserved: `core/` stays Tauri-free; atomic writes in `store.rs`; no `MutexGuard`
held across an `.await` in `commands.rs` (clone/drop before await); neo-brutalist UI; OKF
files stay portable Markdown + YAML.

### Backend

**`src-tauri/src/core/store.rs`** *(sensitive area → security review before commit)*
- Add `pub fn delete_page(&self, rel: &str) -> Result<()>`: removes `{root}/{rel}`, returning
  a contextual error if the file is missing. Editing reuses the existing atomic `write_page`,
  which already overwrites in place.

**`src-tauri/src/commands.rs`** — two new async commands:

- `update_page(state, path, title, tags, note, body) -> Result<PageDto, String>`
  - **Read-modify-write:** `read_page(&path)` first, then overlay the four edited fields onto
    the existing `Frontmatter`/`Page`. This preserves fields the UI does not expose —
    `type_`, `resource`, `timestamp`, and any `extra` frontmatter keys.
  - `write_page` (same path) → `append_log("edited <title>")` → `rebuild_index` (incremental;
    content-hash detects the changed page and re-embeds it) → `index_store::save` →
    `build_link_graph` (body edits may add/remove `[[links]]`) → store new index + links in
    `AppState`. Returns the updated `PageDto`.
- `delete_page(state, path) -> Result<(), String>`
  - Read the page's title (for the log) → `store.delete_page(&path)` →
    `append_log("deleted <title>")` → `rebuild_index` (already drops removed pages) →
    `index_store::save` → `build_link_graph` → store new index + links in `AppState`.

Both clone the needed settings/index out of their mutexes and drop the guards **before** any
`.await`, per the project concurrency rule.

**`src-tauri/src/commands.rs` — `PageViewDto`**
- Add a `body: String` field (the raw page body) so the edit form can seed the textarea from
  a single `get_page_view` fetch rather than a second round-trip. `get_page_view` already
  reads the full `Page`, so this is a one-line addition.

**`src-tauri/src/lib.rs`**
- Register `update_page` and `delete_page` in `generate_handler!`.

### Frontend

**`src/lib/api.ts`**
- `export interface PageView` gains `body: string`.
- `updatePage(path, title, tags, note, body) => invoke<PageDto>("update_page", {...})`
- `deletePage(path) => invoke<void>("delete_page", { path })`

**`src/lib/components/Browse.svelte`**
- Local state: `mode: "view" | "edit"`, `saving`, `editError`, `confirmingDelete`, and the
  four edit fields (`editTitle`, `editTags` as a comma-separated string, `editNote`,
  `editBody`).
- **View mode:** add neo-brutalist **Edit** and **Delete** buttons (`nb-btn`).
- **Edit mode:** replaces the rendered article with a title `<input>`, tags `<input>`
  (comma-separated; split/trim/drop-empties on save), note `<input>`, and a raw-Markdown body
  `<textarea>`. Seeded from the current `PageView`. **Save** → `updatePage`, on success
  reload via `getPageView` and return to view mode; **Cancel** → discard, return to view mode.
  On error, show `editError` and stay in edit mode.
- **Delete:** inline two-step confirm — the Delete button morphs into
  "Confirm delete? [Yes] [No]" (no separate modal; fits the neo-brutalist style). On **Yes**,
  `deletePage` then `currentPage.set(null)` so Browse falls back to the first remaining page,
  or the existing "No pages yet" empty state.

## Data flow

```
Edit:   user edits fields → Save → update_page
          read_page (preserve type/resource/timestamp/extra)
          → write_page (same path, atomic) → append_log("edited …")
          → rebuild_index + save → build_link_graph → AppState
          → return PageDto → Browse reloads getPageView → view mode

Delete: user clicks Delete → Confirm? Yes → delete_page
          read title → store.delete_page → append_log("deleted …")
          → rebuild_index (drops page) + save → build_link_graph → AppState
          → Browse clears currentPage → falls back / empty state
```

## Error handling

- `delete_page` on a missing file → contextual `Err`, surfaced to the UI as a string.
- `update_page` write/index/link failures → `Err` string; the UI keeps the user in edit mode
  with their text intact (`editError` shown). As with `set_settings`, a mid-pipeline failure
  (file written but index rebuild fails) leaves on-disk state ahead of in-memory state; the
  user can re-save or `reindex` to recover. Acceptable for a single-user local app.
- Backlinks to a deleted page already render as red-links via the existing `exists:false`
  segment path — no extra handling needed.

## Testing

**Rust unit — `core/store.rs`**
- `delete_page` removes an existing file.
- `delete_page` on a missing file returns `Err`.

**Rust integration — `tests/commands_integration.rs`**
- Edit round-trip: `update_page` changes body + tags; `get_page_view` reflects them and
  `extra`/`resource`/`timestamp` frontmatter survive.
- Edit updates retrieval: a changed body re-embeds that page in the index.
- Delete: `delete_page` removes the page from `list_pages` and from the index.
- `log.md` gains `- edited <title>` / `- deleted <title>` lines.
- A page linking to a now-deleted page renders the link as a red-link (`exists:false`).

**Frontend — `src/lib/api.test.ts`**
- `updatePage` invokes `"update_page"` with the right args.
- `deletePage` invokes `"delete_page"` with `{ path }`.

**Gates**
- `cargo test` + `cargo clippy --all-targets` + `npm run test` green; `cargo fmt`.
- **Security review on the `store.rs` change before commit** (sensitive area).

## Files touched

| File | Change |
|---|---|
| `src-tauri/src/core/store.rs` | add `delete_page`; unit tests *(sensitive)* |
| `src-tauri/src/commands.rs` | add `update_page`, `delete_page`; add `body` to `PageViewDto` |
| `src-tauri/src/lib.rs` | register the two commands |
| `src-tauri/tests/commands_integration.rs` | edit/delete integration tests |
| `src/lib/api.ts` | `body` on `PageView`; `updatePage`, `deletePage` |
| `src/lib/components/Browse.svelte` | inline edit + delete-confirm UI |
| `src/lib/api.test.ts` | client tests for the two new calls |
