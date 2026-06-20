# M4 Red-link → Create Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Click an unresolved `[[link]]` in Browse to create a stub page at the link's exact slug and open it in the edit form, so the wiki grows organically and the link turns blue.

**Architecture:** Mirrors the existing `update_page`/`delete_page` mutation pattern — write the file via `store.write_page`, then `refresh_index_and_links` (rebuild+persist retrieval index, rebuild link graph), then append to `log.md`. A pure `core::create::new_stub_page` builder, a shared `core::clock::now_iso` timestamp source, one `create_page` Tauri command, and a clickable red-link in `Browse.svelte` that creates then enters edit mode.

**Tech Stack:** Rust + Tauri 2 (core domain + IPC), SvelteKit + Svelte 5 SPA, vitest, cargo test.

**Conventions to follow:**
- Run cargo from `$HOME/.cargo/bin/cargo` (it is not on PATH). Run cargo commands from inside `src-tauri/`.
- `core/` stays Tauri-free (no `tauri::` imports).
- Never hold a `MutexGuard` across an `.await` in `commands.rs` — clone/drop the lock before any `.await`.
- Conventional Commits; end every commit message with exactly:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
- `cargo fmt` and keep `cargo clippy --all-targets` warning-free before each commit.

---

### Task 1: Extract `now_iso` into a shared `core::clock` module

Today `now_iso()` is a private fn in `digest.rs`. The new stub builder (Task 2) needs the same RFC-3339 timestamp. Extract it into a tiny shared module so both call one source (DRY).

**Files:**
- Create: `src-tauri/src/core/clock.rs`
- Modify: `src-tauri/src/core/mod.rs` (add `pub mod clock;`)
- Modify: `src-tauri/src/core/digest.rs` (remove local `now_iso`, call `crate::core::clock::now_iso`, move its test)

- [ ] **Step 1: Create the clock module with its function and test**

Create `src-tauri/src/core/clock.rs`:

```rust
/// Current UTC time as an RFC-3339 string (e.g. `2026-06-20T12:34:56Z`).
/// Falls back to the Unix epoch only if formatting somehow fails.
pub fn now_iso() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_iso_is_rfc3339() {
        let ts = now_iso();
        // RFC-3339 looks like 2026-06-16T12:34:56...Z — 4-digit year then '-', 'T' at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(
            ts.as_bytes()[10],
            b'T',
            "expected date/time 'T' separator, got {ts}"
        );
        assert!(
            !ts.starts_with("unixtime"),
            "should not be the old placeholder, got {ts}"
        );
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/core/mod.rs`, add the module in alphabetical position (between `ask` and `config`):

```rust
pub mod ask;
pub mod clock;
pub mod config;
```

- [ ] **Step 3: Point `digest.rs` at the shared function and delete its local copy**

In `src-tauri/src/core/digest.rs`:

Change the timestamp construction (currently `timestamp: Some(now_iso()),`) to:

```rust
            timestamp: Some(crate::core::clock::now_iso()),
```

Delete the local `now_iso` function:

```rust
fn now_iso() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}
```

Delete its test from the `digest.rs` `#[cfg(test)] mod tests` block (it now lives in `clock.rs`):

```rust
    #[test]
    fn now_iso_is_rfc3339() {
        let ts = now_iso();
        // RFC-3339 looks like 2026-06-16T12:34:56...Z — 4-digit year then '-', 'T' at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(
            ts.as_bytes()[10],
            b'T',
            "expected date/time 'T' separator, got {ts}"
        );
        assert!(
            !ts.starts_with("unixtime"),
            "should not be the old placeholder, got {ts}"
        );
    }
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib clock::`
Expected: PASS — `now_iso_is_rfc3339` runs in its new home.

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib digest::`
Expected: PASS — all digest tests still pass (it now uses `clock::now_iso`).

- [ ] **Step 5: Lint, format, and commit**

```bash
cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
cd .. && git add src-tauri/src/core/clock.rs src-tauri/src/core/mod.rs src-tauri/src/core/digest.rs
git commit -m "$(cat <<'EOF'
refactor: extract now_iso into shared core::clock

Both digest and the upcoming create-stub path need one RFC-3339
timestamp source.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `core::create::new_stub_page` builder

A pure function that builds a stub `Page` from a title: path at the slugified title, `Concept` type, placeholder body, real timestamp. No IO.

**Files:**
- Create: `src-tauri/src/core/create.rs`
- Modify: `src-tauri/src/core/mod.rs` (add `pub mod create;`)

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/core/create.rs`:

```rust
use crate::core::page::{Frontmatter, Page};
use crate::core::slug::slugify;
use std::collections::BTreeMap;

/// Build a minimal "stub" page for a concept that does not exist yet.
///
/// The path is `concepts/{slugify(title)}.md`, so the slug matches what a `[[title]]`
/// red-link resolves against — creating the stub turns that link blue. The body is a
/// visible placeholder the user replaces via the edit flow; no LLM is involved.
pub fn new_stub_page(title: &str) -> Page {
    let slug = slugify(title);
    Page {
        path: format!("concepts/{slug}.md"),
        frontmatter: Frontmatter {
            type_: "Concept".into(),
            title: Some(title.to_string()),
            description: None,
            tags: vec![],
            resource: None,
            timestamp: Some(crate::core::clock::now_iso()),
            note: None,
            extra: BTreeMap::new(),
        },
        body: "_Stub — fill this in._".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_stub_with_slugified_path_and_concept_frontmatter() {
        let p = new_stub_page("Vitamin D & Sleep");
        assert_eq!(p.path, "concepts/vitamin-d-sleep.md");
        assert_eq!(p.frontmatter.type_, "Concept");
        assert_eq!(p.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert!(p.frontmatter.tags.is_empty());
        assert_eq!(p.frontmatter.description, None);
        assert_eq!(p.frontmatter.resource, None);
        assert_eq!(p.frontmatter.note, None);
        assert_eq!(p.body, "_Stub — fill this in._");
    }

    #[test]
    fn stub_has_rfc3339_timestamp() {
        let p = new_stub_page("Alpha");
        let ts = p.frontmatter.timestamp.unwrap();
        // RFC-3339: 4-digit year then '-', 'T' separator at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(ts.as_bytes()[10], b'T', "expected 'T' separator, got {ts}");
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/core/mod.rs`, add `create` in alphabetical position (between `config` and `digest`):

```rust
pub mod config;
pub mod create;
pub mod digest;
```

- [ ] **Step 3: Run the tests and verify they pass**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib create::`
Expected: PASS — both `create::` tests green.

- [ ] **Step 4: Lint, format, and commit**

```bash
cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
cd .. && git add src-tauri/src/core/create.rs src-tauri/src/core/mod.rs
git commit -m "$(cat <<'EOF'
feat: add new_stub_page builder for red-link create

Pure builder for a Concept stub at the slugified title path, with a
placeholder body and a real timestamp.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: `create_page` command + registration + integration tests

The Tauri command: guard against empty/colliding slug, write the stub, log, refresh index+links. Plus integration tests proving a red-link resolves after the stub exists.

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `create_page`; extend the `core::` import + `slugify` import)
- Modify: `src-tauri/src/lib.rs` (register `create_page` in `generate_handler!`)
- Modify: `src-tauri/tests/commands_integration.rs` (add tests; extend imports)

- [ ] **Step 1: Write the failing integration tests**

In `src-tauri/tests/commands_integration.rs`, extend the top imports. The current line is:

```rust
use okf_llm_wiki_lib::core::{ask::ask, digest::digest, store::OkfStore};
```

Replace it with:

```rust
use okf_llm_wiki_lib::core::create::new_stub_page;
use okf_llm_wiki_lib::core::{ask::ask, digest::digest, store::OkfStore};
```

Append these two tests to the end of the file:

```rust
#[tokio::test]
async fn create_stub_resolves_a_previously_red_link() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Seed Beta with a raw [[Ghost]] link. We bypass digest's link validation (which
    // would strip a link to a non-existent concept) by editing the body in directly.
    let b = digest(
        &FakeProvider {
            reply: r#"{"title":"Beta","description":"d","tags":[],"body":"placeholder."}"#.into(),
        },
        "src b",
        None,
        None,
        &[],
    )
    .await
    .unwrap();
    let beta = apply_page_edits(
        b.page,
        Some("Beta".into()),
        vec![],
        None,
        "Beta builds on [[Ghost]].".into(),
    );
    store.write_page(&beta).unwrap();

    // Ghost does not exist yet -> the link is unresolved (red).
    let before = build_link_graph(&store).unwrap();
    assert!(before.path_for("ghost").is_none());

    // Creating the stub at the exact slug resolves the link.
    let ghost = new_stub_page("Ghost");
    assert_eq!(ghost.path, "concepts/ghost.md");
    store.write_page(&ghost).unwrap();
    store.append_log(&format!("created {}", "Ghost")).unwrap();

    let after = build_link_graph(&store).unwrap();
    assert_eq!(after.path_for("ghost"), Some("concepts/ghost.md"));

    // The command appends a "created <title>" line to log.md.
    // Mirrors the command's append_log call; the command path is integration-tested via
    // the composed core primitives since building a Tauri State here is impractical.
    let log = std::fs::read_to_string(dir.join("log.md")).unwrap();
    assert!(log.contains("- created Ghost"));
}

#[tokio::test]
async fn stub_collision_is_detectable_before_overwrite() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // First create succeeds.
    let ghost = new_stub_page("Ghost");
    store.write_page(&ghost).unwrap();

    // The command's collision guard reads the target path; an existing page reads Ok,
    // which is the signal create_page uses to refuse a second create.
    assert!(store.read_page("concepts/ghost.md").is_ok());
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --test commands_integration`
Expected: FAIL — `new_stub_page` is referenced but the import resolves; tests should compile and pass *for the core primitives* even before the command exists, because these tests exercise `new_stub_page` + `build_link_graph` directly (not the command). If they pass here, that is expected — they lock in the core behavior. Proceed to add the command next.

Note: `apply_page_edits` and `build_link_graph` are already imported at the top of this test file from the edit/delete slice. If a `cannot find` error appears for either, add the missing import to the existing `use okf_llm_wiki_lib::core::edit::apply_page_edits;` / `use okf_llm_wiki_lib::core::links::{...};` lines.

- [ ] **Step 3: Add the `create_page` command**

In `src-tauri/src/commands.rs`, extend the `core::` import block to bring in `new_stub_page` and `slugify`. The current import block is:

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

Replace it with:

```rust
use crate::core::{
    ask::ask,
    create::new_stub_page,
    digest::digest,
    edit::apply_page_edits,
    embed::make_embedder,
    fetch::fetch_clean,
    index_store::{self, rebuild_index},
    links::{build_link_graph, segment_body, Segment},
    retrieval::search,
    settings::{make_provider, Settings},
    slug::slugify,
    store::OkfStore,
};
```

Append the command at the end of `src-tauri/src/commands.rs`:

```rust
/// Create a new stub page from a `[[link]]` title and refresh the retrieval index and
/// link graph. The slug is derived from the title, so the originating red-link resolves
/// after this returns. Refuses an empty title or a slug that already exists (no overwrite).
#[tauri::command]
pub async fn create_page(state: State<'_, AppState>, title: String) -> Result<PageDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let slug = slugify(&title);
    if slug.is_empty() {
        return Err("cannot create a page with an empty title".to_string());
    }
    let s = OkfStore::new(settings.wiki_path.clone());
    let path = format!("concepts/{slug}.md");
    if s.read_page(&path).is_ok() {
        return Err(format!("a page for \"{title}\" already exists"));
    }
    let page = new_stub_page(&title);
    s.write_page(&page).map_err(|e| e.to_string())?;
    s.append_log(&format!("created {title}"))
        .map_err(|e| e.to_string())?;
    refresh_index_and_links(&state, &s, &settings).await?;

    Ok(PageDto {
        path: page.path,
        title: page.frontmatter.title.unwrap_or_default(),
        body: page.body,
        tags: page.frontmatter.tags,
        note: page.frontmatter.note,
        resource: page.frontmatter.resource,
    })
}
```

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs`, add `commands::create_page` to the `generate_handler!` list (after `commands::delete_page`):

```rust
            commands::update_page,
            commands::delete_page,
            commands::create_page
        ])
```

- [ ] **Step 5: Run the full Rust suite**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test`
Expected: PASS — unit + integration tests all green, including the two new integration tests.

- [ ] **Step 6: Lint, format, and commit**

```bash
cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt
cd .. && git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/tests/commands_integration.rs
git commit -m "$(cat <<'EOF'
feat: add create_page command for red-link stubs

Guards empty/colliding slugs, writes a stub, logs, and refreshes the
index + link graph so the originating red-link resolves.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: `createPage` API client + test

**Files:**
- Modify: `src/lib/api.ts` (add `createPage`)
- Modify: `src/lib/api.test.ts` (add a test; extend the import line)

- [ ] **Step 1: Write the failing test**

In `src/lib/api.test.ts`, extend the import line. The current line is:

```ts
import { listPages, submitSource, setSettings, getPageView, reindex, updatePage, deletePage } from "./api";
```

Replace it with:

```ts
import { listPages, submitSource, setSettings, getPageView, reindex, updatePage, deletePage, createPage } from "./api";
```

Add this test inside the `describe("api", ...)` block (after the `deletePage` test):

```ts
  it("createPage invokes the create_page command", async () => {
    await createPage("Ghost");
    expect(invoke).toHaveBeenCalledWith("create_page", { title: "Ghost" });
  });
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm run test`
Expected: FAIL — `createPage` is not exported from `./api`.

- [ ] **Step 3: Add the client**

In `src/lib/api.ts`, append after the `deletePage` export:

```ts
export const createPage = (title: string) => invoke<PageDto>("create_page", { title });
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm run test`
Expected: PASS — all vitest tests green, including the new `createPage` test.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/lib/api.test.ts
git commit -m "$(cat <<'EOF'
feat: add createPage API client

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Clickable red-link → create + open in edit (`Browse.svelte`)

Make the red-link actionable: clicking it calls `createPage(seg.text)`, navigates to the new page, and opens it in edit mode. Show errors as a neo-brutalist card.

**Files:**
- Modify: `src/lib/components/Browse.svelte`

- [ ] **Step 1: Import `createPage`**

In `src/lib/components/Browse.svelte`, the current import line is:

```ts
  import { listPages, getPageView, updatePage, deletePage, type PageDto, type PageView } from "$lib/api";
```

Replace it with:

```ts
  import { listPages, getPageView, updatePage, deletePage, createPage, type PageDto, type PageView } from "$lib/api";
```

- [ ] **Step 2: Add create state**

After the existing `let deleteError = "";` line, add:

```ts
  let creating = false;
  let createError = "";
  let pendingEdit = false;
```

- [ ] **Step 3: Consume `pendingEdit` and reset `createError` in `loadFor`**

The current `loadFor` is:

```ts
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    view = await getPageView(path);
    mode = "view";
    confirmingDelete = false;
    deleteError = "";
  }
```

Replace it with:

```ts
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    view = await getPageView(path);
    mode = "view";
    confirmingDelete = false;
    deleteError = "";
    createError = "";
    if (pendingEdit) { pendingEdit = false; startEdit(); }
  }
```

- [ ] **Step 4: Add the `createFromLink` handler**

After the `cancelDelete` function (`function cancelDelete() { confirmingDelete = false; }`), add:

```ts
  async function createFromLink(title: string) {
    creating = true;
    createError = "";
    try {
      const p = await createPage(title);
      pages = await listPages();
      pendingEdit = true;
      currentPage.set(p.path);
    } catch (e) {
      createError = String(e);
      creating = false;
    }
  }
```

Note: on success we do **not** reset `creating` here — navigation triggers `loadFor`, which re-renders into edit mode; leaving `creating` true until the component reactively reloads avoids a double-click. On error we reset it so the user can retry.

- [ ] **Step 5: Render the create error card and make the red-link clickable**

In view mode, after the existing delete-error card line:

```svelte
    {#if deleteError}<div class="nb-card" style="background:#c0392b;color:#fff;margin:0 0 8px 0">{deleteError}</div>{/if}
```

add a create-error card:

```svelte
    {#if createError}<div class="nb-card" style="background:#c0392b;color:#fff;margin:0 0 8px 0">{createError}</div>{/if}
```

In the article's segment loop, the current red-link branch is:

```svelte
{:else if seg.kind === "link"}<span class="nb-redlink" title="Page not found">{seg.text}</span>
```

Replace it with a clickable element (keeps the `nb-redlink` look, adds the create action):

```svelte
{:else if seg.kind === "link"}<a class="nb-redlink" href="#/" title="Create this page" on:click|preventDefault={() => createFromLink(seg.text)}>{seg.text}</a>
```

- [ ] **Step 6: Update the red-link style so the anchor stays inline and shows intent**

In the `<style>` block, the current `.nb-redlink` rule is:

```css
  .nb-redlink {
    color: #c0392b;
    font-weight: 700;
    text-decoration: underline dotted;
    cursor: not-allowed;
  }
```

Replace it with:

```css
  .nb-redlink {
    color: #c0392b;
    font-weight: 700;
    text-decoration: underline dotted;
    cursor: pointer;
  }
```

- [ ] **Step 7: Type-check and build**

Run: `npm run check`
Expected: 0 errors.

Run: `npm run build`
Expected: build succeeds (adapter-static → `build/`).

- [ ] **Step 8: Commit**

```bash
git add src/lib/components/Browse.svelte
git commit -m "$(cat <<'EOF'
feat: clickable red-link creates a stub and opens it in edit

Clicking an unresolved [[link]] calls create_page, navigates to the new
stub, and enters edit mode so the user can fill it in.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Final verification (after all tasks)

- [ ] `cd src-tauri && $HOME/.cargo/bin/cargo test` — all unit + integration tests pass.
- [ ] `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets` — warning-free.
- [ ] `cd src-tauri && $HOME/.cargo/bin/cargo fmt --check` — formatted.
- [ ] `npm run test` — vitest green.
- [ ] `npm run check` — 0 errors.
- [ ] `npm run build` — succeeds.
