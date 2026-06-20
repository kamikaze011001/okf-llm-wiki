# M4 Edit & Delete Pages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user edit (body, title, tags, note) and delete OKF pages from inside the app, keeping the retrieval index, link graph, and `log.md` correct.

**Architecture:** Two new async Tauri commands (`update_page`, `delete_page`) follow the existing `submit_source` mutation pattern — mutate the file, then rebuild + persist the retrieval index, rebuild the link graph, and append to `log.md`. The correctness-critical read-modify-write merge lives in a small pure `core::edit` helper (unit-testable, Tauri-free). The page path/slug never changes, so `[[links]]` and backlinks stay valid. Frontend adds an inline edit toggle and a two-step delete confirm to `Browse.svelte`.

**Tech Stack:** Rust + Tauri 2 (core + IPC), SvelteKit + Svelte 5 SPA, `cargo test` + vitest.

**Reference spec:** `docs/superpowers/specs/2026-06-20-m4-edit-delete-pages-design.md`

---

## Conventions for every task

- Run Rust commands from `src-tauri/` with the cargo on PATH at `$HOME/.cargo/bin/cargo`. `cd` does not persist between shells — use a compound command, e.g.
  `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo test <name>`.
- `core/` must stay Tauri-free (no `tauri::` imports under `src-tauri/src/core/`).
- In `commands.rs`, never hold a `MutexGuard` across an `.await`: clone/drop the lock at the statement before any `.await`.
- Conventional Commits; end every commit message with exactly:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
- **Task 1 touches `src-tauri/src/core/store.rs`, a sensitive area.** Do NOT commit it — `git add` the changes and STOP; the controller runs a security review and commits. All other tasks commit normally.

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `src-tauri/src/core/store.rs` | atomic file IO; gains `delete_page` *(sensitive)* | 1 |
| `src-tauri/src/core/edit.rs` *(new)* | pure read-modify-write merge of edited fields onto a `Page` | 2 |
| `src-tauri/src/core/mod.rs` | declare the new `edit` module | 2 |
| `src-tauri/src/commands.rs` | `update_page`, `delete_page` commands; `body` on `PageViewDto` | 3 |
| `src-tauri/src/lib.rs` | register the two new commands | 3 |
| `src-tauri/tests/commands_integration.rs` | end-to-end edit/delete flow over core building blocks | 3 |
| `src/lib/api.ts` | `body` on `PageView`; `updatePage`, `deletePage` clients | 4 |
| `src/lib/api.test.ts` | client invoke tests | 4 |
| `src/lib/components/Browse.svelte` | inline edit + delete-confirm UI | 5 |

---

### Task 1: `store.delete_page` (sensitive area)

**Files:**
- Modify: `src-tauri/src/core/store.rs`
- Test: `src-tauri/src/core/store.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the existing `mod tests` block in `src-tauri/src/core/store.rs` (after `reads_page_without_trailing_newline`):

```rust
    #[test]
    fn delete_page_removes_file() {
        let root = tmp();
        let store = OkfStore::new(root.clone());
        let page = Page {
            path: "concepts/gone.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Gone".into()),
                description: None,
                tags: vec![],
                resource: None,
                timestamp: None,
                note: None,
                extra: BTreeMap::new(),
            },
            body: "bye".into(),
        };
        store.write_page(&page).unwrap();
        assert!(root.join("concepts/gone.md").exists());
        store.delete_page("concepts/gone.md").unwrap();
        assert!(!root.join("concepts/gone.md").exists());
    }

    #[test]
    fn delete_page_missing_file_errors() {
        let store = OkfStore::new(tmp());
        assert!(store.delete_page("concepts/nope.md").is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo test delete_page 2>&1 | tail -20`
Expected: FAIL to compile with "no method named `delete_page`".

- [ ] **Step 3: Implement `delete_page`**

Add this method to `impl OkfStore` in `src-tauri/src/core/store.rs`, immediately after `read_page`:

```rust
    /// Delete a page file. Errors if it does not exist.
    pub fn delete_page(&self, rel: &str) -> Result<()> {
        let path = self.root.join(rel);
        std::fs::remove_file(&path)
            .with_context(|| format!("deleting {}", path.display()))?;
        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo test delete_page 2>&1 | tail -20`
Expected: PASS (`delete_page_removes_file`, `delete_page_missing_file_errors`).

- [ ] **Step 5: Format, then STAGE (do not commit — sensitive area)**

```bash
cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo fmt
cd "$HOME/Documents/AIBLES/okf-llm-wiki" && git add src-tauri/src/core/store.rs
```

Report DONE and that the change is staged but **not committed** (store.rs is a sensitive area; the controller will run a security review and commit).

---

### Task 2: `core::edit::apply_page_edits` pure merge helper

**Files:**
- Create: `src-tauri/src/core/edit.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Test: `src-tauri/src/core/edit.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Declare the module**

In `src-tauri/src/core/mod.rs`, add this line in alphabetical position (between `pub mod digest;` and `pub mod embed;`):

```rust
pub mod edit;
```

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/core/edit.rs` with:

```rust
use crate::core::page::Page;

/// Apply edited fields onto an existing page, preserving every frontmatter field
/// the editor does not expose (`type`, `description`, `resource`, `timestamp`, and any
/// flattened `extra` keys). The page path/slug is never changed here.
pub fn apply_page_edits(
    mut page: Page,
    title: Option<String>,
    tags: Vec<String>,
    note: Option<String>,
    body: String,
) -> Page {
    page.frontmatter.title = title;
    page.frontmatter.tags = tags;
    page.frontmatter.note = note;
    page.body = body;
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::page::Frontmatter;
    use std::collections::BTreeMap;

    fn sample() -> Page {
        let mut extra = BTreeMap::new();
        extra.insert(
            "okf_version".to_string(),
            serde_yaml::Value::String("0.1".into()),
        );
        Page {
            path: "concepts/x.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Old".into()),
                description: Some("desc".into()),
                tags: vec!["old".into()],
                resource: Some("https://src".into()),
                timestamp: Some("2026-01-01T00:00:00Z".into()),
                note: Some("old note".into()),
                extra,
            },
            body: "old body".into(),
        }
    }

    #[test]
    fn updates_exposed_fields() {
        let p = apply_page_edits(
            sample(),
            Some("New".into()),
            vec!["a".into(), "b".into()],
            Some("new note".into()),
            "new body".into(),
        );
        assert_eq!(p.frontmatter.title, Some("New".into()));
        assert_eq!(p.frontmatter.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(p.frontmatter.note, Some("new note".into()));
        assert_eq!(p.body, "new body");
    }

    #[test]
    fn preserves_hidden_fields_and_path() {
        let p = apply_page_edits(sample(), Some("New".into()), vec![], None, "b".into());
        assert_eq!(p.path, "concepts/x.md");
        assert_eq!(p.frontmatter.type_, "Concept");
        assert_eq!(p.frontmatter.description, Some("desc".into()));
        assert_eq!(p.frontmatter.resource, Some("https://src".into()));
        assert_eq!(
            p.frontmatter.timestamp,
            Some("2026-01-01T00:00:00Z".into())
        );
        assert_eq!(
            p.frontmatter.extra.get("okf_version"),
            Some(&serde_yaml::Value::String("0.1".into()))
        );
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

The implementation is in the same file as the tests, so this is green on first run.
Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo test apply_page_edits 2>&1 | tail -20`
Expected: PASS — but FIRST confirm TDD discipline by temporarily commenting out the four assignment lines in `apply_page_edits` and re-running to see `updates_exposed_fields` FAIL, then restore them.

- [ ] **Step 4: Run the full core suite + clippy**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo fmt && $HOME/.cargo/bin/cargo clippy --all-targets 2>&1 | tail -5 && $HOME/.cargo/bin/cargo test 2>&1 | tail -5`
Expected: clippy clean; all tests pass.

- [ ] **Step 5: Commit**

```bash
cd "$HOME/Documents/AIBLES/okf-llm-wiki" && git add src-tauri/src/core/edit.rs src-tauri/src/core/mod.rs && git commit -m "$(printf 'feat: add core::edit::apply_page_edits merge helper\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

### Task 3: `update_page` + `delete_page` commands and integration tests

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs:34-42`
- Test: `src-tauri/tests/commands_integration.rs`

The Tauri command functions take `State<AppState>`, which is impractical to build in a test, so the commands stay thin and delegate to already-tested core primitives (`apply_page_edits`, `store.delete_page`, `rebuild_index`, `build_link_graph`). The integration test verifies the **composed flow** over those primitives, mirroring exactly what the commands do.

- [ ] **Step 1: Write the failing integration tests**

Append to `src-tauri/tests/commands_integration.rs`. First extend the imports at the top of the file so they read:

```rust
use okf_llm_wiki_lib::core::edit::apply_page_edits;
use okf_llm_wiki_lib::core::embed::HashEmbedder;
use okf_llm_wiki_lib::core::index_store::{rebuild_index, PersistedIndex};
use okf_llm_wiki_lib::core::links::{build_link_graph, concept_refs, segment_body, BacklinkRef, Segment};
use okf_llm_wiki_lib::core::provider::fake::FakeProvider;
use okf_llm_wiki_lib::core::retrieval::search;
use okf_llm_wiki_lib::core::{ask::ask, digest::digest, store::OkfStore};
```

Then append these tests at the end of the file:

```rust
#[tokio::test]
async fn edit_round_trip_preserves_hidden_frontmatter_and_updates_index() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Seed a page with a resource + body, via digest (so frontmatter is realistic).
    let r = digest(
        &FakeProvider {
            reply: r#"{"title":"Alpha","description":"d","tags":["old"],"body":"**TL;DR.** old."}"#.into(),
        },
        "src",
        Some("https://src"),
        Some("orig note"),
        &[],
    )
    .await
    .unwrap();
    store.write_page(&r.page).unwrap();
    let path = r.page.path.clone();

    let embedder = HashEmbedder;
    let before = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    let hash_before = before.pages.get(&path).unwrap().content_hash;

    // Simulate update_page's core flow.
    let existing = store.read_page(&path).unwrap();
    let edited = apply_page_edits(
        existing,
        Some("Alpha".into()),
        vec!["new".into()],
        Some("orig note".into()),
        "**TL;DR.** brand new body.".into(),
    );
    store.write_page(&edited).unwrap();

    // Read back: edited fields applied, hidden frontmatter (resource) preserved.
    let read = store.read_page(&path).unwrap();
    assert_eq!(read.frontmatter.tags, vec!["new".to_string()]);
    assert!(read.body.contains("brand new body"));
    assert_eq!(read.frontmatter.resource, Some("https://src".into()));

    // The command appends an "edited <title>" line to log.md.
    let log_title = read.frontmatter.title.clone().unwrap_or_default();
    store.append_log(&format!("edited {log_title}")).unwrap();
    let log = std::fs::read_to_string(dir.join("log.md")).unwrap();
    assert!(log.contains(&format!("- edited {log_title}")));

    // Index reflects the changed body (content hash differs -> re-embedded).
    let after = rebuild_index(&store, &embedder, &before).await.unwrap();
    let hash_after = after.pages.get(&path).unwrap().content_hash;
    assert_ne!(hash_before, hash_after);
}

#[tokio::test]
async fn delete_drops_page_from_listing_and_index() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);
    let r = digest(
        &FakeProvider {
            reply: r#"{"title":"Alpha","description":"d","tags":[],"body":"**TL;DR.** a."}"#.into(),
        },
        "src",
        None,
        None,
        &[],
    )
    .await
    .unwrap();
    store.write_page(&r.page).unwrap();
    let path = r.page.path.clone();

    let embedder = HashEmbedder;
    let before = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    assert!(before.pages.contains_key(&path));

    store.delete_page(&path).unwrap();

    assert!(!store.list_pages().unwrap().contains(&path));
    // rebuild_index drops pages no longer on disk.
    let after = rebuild_index(&store, &embedder, &before).await.unwrap();
    assert!(!after.pages.contains_key(&path));

    // The command appends a "deleted <title>" line to log.md.
    store.append_log("deleted Alpha").unwrap();
    let log = std::fs::read_to_string(dir.join("log.md")).unwrap();
    assert!(log.contains("- deleted Alpha"));
}

#[tokio::test]
async fn link_to_deleted_page_renders_as_red_link() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Alpha exists; Beta links to it.
    let a = digest(
        &FakeProvider {
            reply: r#"{"title":"Alpha","description":"d","tags":[],"body":"**TL;DR.** a."}"#.into(),
        },
        "src a",
        None,
        None,
        &[],
    )
    .await
    .unwrap();
    store.write_page(&a.page).unwrap();
    let existing = concept_refs(&store).unwrap();
    let b = digest(
        &FakeProvider {
            reply: r#"{"title":"Beta","description":"d","tags":[],"body":"Beta builds on [[Alpha]]."}"#.into(),
        },
        "src b",
        None,
        None,
        &existing,
    )
    .await
    .unwrap();
    store.write_page(&b.page).unwrap();

    // Delete Alpha, rebuild the graph.
    store.delete_page("concepts/alpha.md").unwrap();
    let graph = build_link_graph(&store).unwrap();

    // Beta's [[Alpha]] link now resolves to nothing -> red-link (exists == false).
    let beta = store.read_page("concepts/beta.md").unwrap();
    let has_unresolved_link = segment_body(&beta.body).iter().any(|seg| {
        matches!(seg, Segment::Link { target_slug, .. } if graph.path_for(target_slug).is_none())
    });
    assert!(has_unresolved_link, "deleted target should be unresolved");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo test --test commands_integration 2>&1 | tail -25`
Expected: FAIL to compile — `apply_page_edits`/`segment_body`/`Segment` import paths resolve (from Tasks 1–2), but if Task 1's `delete_page` is not yet merged into the working tree, `store.delete_page` won't exist. (It is — Tasks run in order.) The genuine new-failure signal here is the assertions; if everything compiles, the tests should pass since the primitives exist. If they pass immediately, that is acceptable for an integration test composed of pre-built primitives — proceed.

- [ ] **Step 3: Add `body` to `PageViewDto` and implement the two commands**

In `src-tauri/src/commands.rs`:

(a) Add `body` to the `PageViewDto` struct (after the `title` field):

```rust
#[derive(Serialize)]
pub struct PageViewDto {
    pub path: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub resource: Option<String>,
    pub segments: Vec<SegmentDto>,
    pub backlinks: Vec<RefDto>,
}
```

(b) In `get_page_view`, set the new field. Change the returned struct literal so it includes `body: page.body.clone(),` — note `page.body` is moved into `segment_body` first, so clone before segmenting. Replace the line `let segments = segment_body(&page.body)` region's surrounding return so the final `Ok(PageViewDto { ... })` includes `body: page.body.clone(),`. Concretely, add `let body = page.body.clone();` right after `let page = s.read_page(&path).map_err(|e| e.to_string())?;` and add `body,` to the returned `PageViewDto`.

(c) Extend the `use crate::core::{...}` block at the top to include `edit::apply_page_edits`:

```rust
use crate::core::{
    ask::ask,
    digest::digest,
    edit::apply_page_edits,
    embed::make_embedder,
    fetch::fetch_clean,
    index_store::{self, rebuild_index},
    links::{build_link_graph, segment_body, Segment},
    retrieval::search,
    settings::{make_provider, Settings},
    store::OkfStore,
};
```

(d) Add the two commands at the end of `src-tauri/src/commands.rs`. They mirror `submit_source`'s refresh pattern and never hold a guard across `.await`:

```rust
/// Edit a page's body/title/tags/note in place (path unchanged), then refresh the
/// retrieval index and link graph. Hidden frontmatter is preserved via read-modify-write.
#[tauri::command]
pub async fn update_page(
    state: State<'_, AppState>,
    path: String,
    title: Option<String>,
    tags: Vec<String>,
    note: Option<String>,
    body: String,
) -> Result<PageDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let s = OkfStore::new(settings.wiki_path.clone());
    let existing = s.read_page(&path).map_err(|e| e.to_string())?;
    let edited = apply_page_edits(existing, title, tags, note, body);
    s.write_page(&edited).map_err(|e| e.to_string())?;
    let log_title = edited.frontmatter.title.clone().unwrap_or_default();
    s.append_log(&format!("edited {log_title}"))
        .map_err(|e| e.to_string())?;

    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let prev = state.index.lock().unwrap().clone();
    let next = rebuild_index(&s, embedder.as_ref(), &prev)
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = next;
    *state.links.lock().unwrap() = build_link_graph(&s).map_err(|e| e.to_string())?;

    Ok(PageDto {
        path: edited.path,
        title: edited.frontmatter.title.unwrap_or_default(),
        body: edited.body,
        tags: edited.frontmatter.tags,
        note: edited.frontmatter.note,
        resource: edited.frontmatter.resource,
    })
}

/// Delete a page, then refresh the retrieval index and link graph.
#[tauri::command]
pub async fn delete_page(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    let s = OkfStore::new(settings.wiki_path.clone());
    let title = s
        .read_page(&path)
        .ok()
        .and_then(|p| p.frontmatter.title)
        .unwrap_or_default();
    s.delete_page(&path).map_err(|e| e.to_string())?;
    s.append_log(&format!("deleted {title}"))
        .map_err(|e| e.to_string())?;

    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let prev = state.index.lock().unwrap().clone();
    let next = rebuild_index(&s, embedder.as_ref(), &prev)
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = next;
    *state.links.lock().unwrap() = build_link_graph(&s).map_err(|e| e.to_string())?;

    Ok(())
}
```

- [ ] **Step 4: Register the commands in `lib.rs`**

In `src-tauri/src/lib.rs`, change the `generate_handler!` list (lines 34-42) to append the two commands after `commands::reindex`:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_pages,
            commands::get_page_view,
            commands::submit_source,
            commands::ask_question,
            commands::reindex,
            commands::update_page,
            commands::delete_page
        ])
```

- [ ] **Step 5: Run tests + clippy to verify green**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo fmt && $HOME/.cargo/bin/cargo clippy --all-targets 2>&1 | tail -5 && $HOME/.cargo/bin/cargo test 2>&1 | tail -8`
Expected: clippy clean; all unit + integration tests pass (including the three new integration tests).

- [ ] **Step 6: Commit**

```bash
cd "$HOME/Documents/AIBLES/okf-llm-wiki" && git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/tests/commands_integration.rs && git commit -m "$(printf 'feat: add update_page and delete_page commands\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

### Task 4: Frontend API client (`updatePage`, `deletePage`, `body` on `PageView`)

**Files:**
- Modify: `src/lib/api.ts`
- Test: `src/lib/api.test.ts`

- [ ] **Step 1: Write the failing tests**

Open `src/lib/api.test.ts`. Add `updatePage` and `deletePage` to the import from `$lib/api` (alongside the existing imports), then add these tests:

```ts
test("updatePage invokes the update_page command", async () => {
  await updatePage("concepts/a.md", "New Title", ["x", "y"], "note", "new body");
  expect(invoke).toHaveBeenCalledWith("update_page", {
    path: "concepts/a.md",
    title: "New Title",
    tags: ["x", "y"],
    note: "note",
    body: "new body",
  });
});

test("deletePage invokes the delete_page command", async () => {
  await deletePage("concepts/a.md");
  expect(invoke).toHaveBeenCalledWith("delete_page", { path: "concepts/a.md" });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki" && npm run test 2>&1 | tail -20`
Expected: FAIL — `updatePage`/`deletePage` are not exported from `$lib/api`.

- [ ] **Step 3: Implement the client functions and interface field**

In `src/lib/api.ts`:

(a) Add `body: string;` to the `PageView` interface (after `title: string;`):

```ts
export interface PageView { path: string; title: string; body: string; tags: string[]; note?: string; resource?: string; segments: Segment[]; backlinks: Ref[]; }
```

(b) Add the two client functions after the existing `reindex` export:

```ts
export const updatePage = (path: string, title: string | undefined, tags: string[], note: string | undefined, body: string) =>
  invoke<PageDto>("update_page", { path, title, tags, note, body });
export const deletePage = (path: string) => invoke<void>("delete_page", { path });
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki" && npm run test 2>&1 | tail -12`
Expected: PASS — all vitest tests green (5 existing + 2 new = 7).

- [ ] **Step 5: Commit**

```bash
cd "$HOME/Documents/AIBLES/okf-llm-wiki" && git add src/lib/api.ts src/lib/api.test.ts && git commit -m "$(printf 'feat: add updatePage/deletePage API clients\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

### Task 5: Inline edit + delete UI in `Browse.svelte`

**Files:**
- Modify: `src/lib/components/Browse.svelte`

There is no component test harness in this project, so verification is `npm run check` (type-safety) plus the build. Follow the existing legacy Svelte style in this file (`let`, `$:`, `on:click`) for consistency.

- [ ] **Step 1: Replace the `<script>` block**

Replace the entire `<script lang="ts"> ... </script>` block at the top of `src/lib/components/Browse.svelte` with:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, getPageView, updatePage, deletePage, type PageDto, type PageView } from "$lib/api";
  import { currentPage } from "$lib/stores";
  let pages: PageDto[] = [];
  let view: PageView | undefined;
  let mounted = false;
  let mode: "view" | "edit" = "view";
  let saving = false;
  let editError = "";
  let confirmingDelete = false;
  // Edit form fields (seeded from `view` when entering edit mode).
  let editTitle = "";
  let editTags = "";
  let editNote = "";
  let editBody = "";
  onMount(async () => { pages = await listPages(); mounted = true; });
  $: selectedPath = $currentPage ?? pages[0]?.path ?? null;
  $: if (mounted) loadFor(selectedPath);
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    view = await getPageView(path);
    mode = "view";
    confirmingDelete = false;
  }
  function go(path: string) { currentPage.set(path); }
  function startEdit() {
    if (!view) return;
    editTitle = view.title;
    editTags = view.tags.join(", ");
    editNote = view.note ?? "";
    editBody = view.body;
    editError = "";
    mode = "edit";
  }
  async function saveEdit() {
    if (!view) return;
    saving = true;
    editError = "";
    try {
      const tags = editTags.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
      await updatePage(view.path, editTitle || undefined, tags, editNote || undefined, editBody);
      view = await getPageView(view.path);
      mode = "view";
    } catch (e) {
      editError = String(e);
    } finally {
      saving = false;
    }
  }
  async function confirmDelete() {
    if (!view) return;
    await deletePage(view.path);
    pages = await listPages();
    confirmingDelete = false;
    currentPage.set(null);
  }
  function cancelDelete() { confirmingDelete = false; }
</script>
```

- [ ] **Step 2: Replace the view-mode markup with view/edit modes**

Replace the `<section>...</section>` block (the whole template, not the `<style>`) with the following. It keeps the existing read-only rendering for `view` mode and adds an edit form plus action buttons:

```svelte
<section style="padding:32px;max-width:760px;margin:0 auto">
  {#if view && mode === "view"}
    <div style="display:flex;gap:8px;justify-content:flex-end;margin-bottom:8px">
      <button class="nb-btn" on:click={startEdit}>Edit</button>
      {#if confirmingDelete}
        <span style="align-self:center">Confirm delete?</span>
        <button class="nb-btn" style="background:#c0392b;color:#fff" on:click={confirmDelete}>Yes</button>
        <button class="nb-btn" on:click={cancelDelete}>No</button>
      {:else}
        <button class="nb-btn" on:click={() => (confirmingDelete = true)}>Delete</button>
      {/if}
    </div>
    <span class="nb-chip" style="background:var(--pink);color:#fff">CONCEPT</span>
    <h1>{view.title}</h1>
    <div>{#each view.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
    {#if view.note}<div class="nb-card" style="background:var(--yellow);margin:12px 0"><strong>★ Your note:</strong> {view.note}</div>{/if}
    <article class="nb-card" style="margin-top:12px;white-space:pre-wrap">{#each view.segments as seg}{#if seg.kind === "link" && seg.exists}<a class="nb-wikilink" href="#/" on:click|preventDefault={() => go(seg.target_path!)}>{seg.text}</a>{:else if seg.kind === "link"}<span class="nb-redlink" title="Page not found">{seg.text}</span>{:else}{seg.text}{/if}{/each}</article>
    {#if view.resource}<p style="margin-top:12px"><a href={view.resource} target="_blank">Open source ↗</a></p>{/if}
    {#if view.backlinks.length}
      <div class="nb-card" style="margin-top:16px">
        <strong>Linked from</strong>
        <ul style="margin:8px 0 0 0;padding-left:20px">
          {#each view.backlinks as b}<li><a class="nb-wikilink" href="#/" on:click|preventDefault={() => go(b.path)}>{b.title}</a></li>{/each}
        </ul>
      </div>
    {/if}
  {:else if view && mode === "edit"}
    <h2>Edit page</h2>
    {#if editError}<div class="nb-card" style="background:#c0392b;color:#fff;margin:8px 0">{editError}</div>{/if}
    <label style="display:block;margin-top:8px">Title<br /><input class="nb-input" style="width:100%" bind:value={editTitle} /></label>
    <label style="display:block;margin-top:8px">Tags (comma-separated)<br /><input class="nb-input" style="width:100%" bind:value={editTags} /></label>
    <label style="display:block;margin-top:8px">Note<br /><input class="nb-input" style="width:100%" bind:value={editNote} /></label>
    <label style="display:block;margin-top:8px">Body (Markdown)<br /><textarea class="nb-input" style="width:100%;min-height:240px;font-family:monospace" bind:value={editBody}></textarea></label>
    <div style="display:flex;gap:8px;margin-top:12px">
      <button class="nb-btn" on:click={saveEdit} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
      <button class="nb-btn" on:click={() => (mode = "view")} disabled={saving}>Cancel</button>
    </div>
  {:else}
    <p>No pages yet — capture something from Home.</p>
  {/if}
</section>
```

- [ ] **Step 3: Verify types and build**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki" && npm run check 2>&1 | tail -15`
Expected: 0 errors. If `nb-input`/`nb-btn` classes are undefined that is fine — they are global neo-brutalist classes; `svelte-check` only flags TypeScript/Svelte errors, not unknown CSS classes.

- [ ] **Step 4: Build to confirm the SPA compiles**

Run: `cd "$HOME/Documents/AIBLES/okf-llm-wiki" && npm run build 2>&1 | tail -8`
Expected: build succeeds (exit 0), `build/` emitted.

- [ ] **Step 5: Commit**

```bash
cd "$HOME/Documents/AIBLES/okf-llm-wiki" && git add src/lib/components/Browse.svelte && git commit -m "$(printf 'feat: inline edit and delete UI in Browse\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

## Final verification (after all tasks)

- [ ] `cd "$HOME/Documents/AIBLES/okf-llm-wiki/src-tauri" && $HOME/.cargo/bin/cargo fmt && $HOME/.cargo/bin/cargo clippy --all-targets 2>&1 | tail -5 && $HOME/.cargo/bin/cargo test 2>&1 | tail -8` — clippy clean, all Rust tests pass.
- [ ] `cd "$HOME/Documents/AIBLES/okf-llm-wiki" && npm run check && npm run test && npm run build` — types clean, vitest green, build succeeds.
- [ ] Security review on the `store.rs` change (Task 1) before it is committed.
- [ ] Final code review across the whole branch, then `superpowers:finishing-a-development-branch`.
