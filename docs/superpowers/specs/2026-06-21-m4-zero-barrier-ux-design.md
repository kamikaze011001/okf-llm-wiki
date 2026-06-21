# M4 Zero-Barrier UX — Design

**Status:** Approved 2026-06-21
**Milestone:** M4 (Zero-barrier UX) — the remaining slice after edit/delete, red-link create, and the concept graph
**Branch:** `feat/m4-zero-barrier-ux`

## 1. Summary

Three loosely-coupled UX pieces, shipped together in one spec because they are all
**frontend-only** and share the same neo-brutalist primitives:

1. **First-run onboarding** — a hard gate that shows a setup wizard until the app is
   configured (API key + wiki folder), so a fresh install never lands on a broken,
   unconfigured main UI.
2. **Empty & loading states** — real empty states and indeterminate loading feedback
   across every view, replacing today's bare text and blank panes.
3. **Frictionless capture** — three plugin-free entry points (in-app shortcut, drag-drop,
   paste-anywhere) that route a link/note straight into the capture flow.

## 2. Decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Onboarding behavior | **Hard gate** — block the main UI until API key + wiki folder are set |
| Onboarding form | **Single screen** (not a multi-step wizard) — only key + folder are required |
| Capture hotkey | **In-app shortcut only** (`Cmd/Ctrl+N`) — no global-shortcut plugin, no tray/background |
| Drag-drop inputs | **Links/text only** — no file reading, no new file IO |
| Loading/progress | **Indeterminate, frontend-only** — no backend events, `submit_source` untouched |
| Capture drop/paste scope | **Global** — drop/paste anywhere in the window routes to capture |

## 3. Scope boundary — no backend, no new surface

All work is in `src/`. **No** Rust changes, **no** new Tauri commands, **no** new Tauri
plugins or capabilities, **no** changes to `tauri.conf.json` permissions. None of the
sensitive areas (`settings.rs`, `state.rs`, `store.rs`, `provider/`, `embed/`,
`index_store.rs`) are touched. **No security review is required for this slice.**

The frontend reuses existing IPC only: `getSettings`, `setSettings`, `listPages`,
`submitSource`, `getPageView`, `askQuestion`, `getGraph`.

## 4. File structure

| File | Change | Responsibility |
|---|---|---|
| `src/lib/capture.ts` | **New** | Pure helpers: `isConfigured`, `extractDropText`, `extractPasteText`, `shouldInterceptPaste` |
| `src/lib/capture.test.ts` | **New** | Vitest for the pure helpers |
| `src/lib/stores.ts` | Modify | Add `configured` + `capturePrefill` writable stores |
| `src/lib/components/Spinner.svelte` | **New** | Neo-brutalist indeterminate busy indicator |
| `src/lib/components/EmptyState.svelte` | **New** | Neo-brutalist empty-state card (title + subtext + optional action) |
| `src/lib/components/Onboarding.svelte` | **New** | First-run single-screen setup form |
| `src/routes/+page.svelte` | Modify | Onboarding gate + window-level capture handlers |
| `src/lib/components/Home.svelte` | Modify | Consume `capturePrefill`; empty "Recent"; loading states |
| `src/lib/components/Browse.svelte` | Modify | Empty + loading states |
| `src/lib/components/Ask.svelte` | Modify | Empty + loading states |
| `src/lib/components/Graph.svelte` | Modify | Standardize its empty state onto `EmptyState` |

## 5. `src/lib/capture.ts` — pure helpers (testable)

```ts
import type { Settings } from "./api";

/** The app is usable only once a key and a wiki folder are set. */
export function isConfigured(s: Settings): boolean {
  return s.api_key.trim() !== "" && s.wiki_path.trim() !== "";
}

/** Extract a droppable link/text from a DataTransfer. Prefers a URL, falls back to text. */
export function extractDropText(dt: Pick<DataTransfer, "getData"> | null): string {
  if (!dt) return "";
  const uri = dt.getData("text/uri-list");
  if (uri) {
    // text/uri-list may contain comment lines starting with '#'; take the first real URL.
    const first = uri.split(/\r?\n/).find((l) => l && !l.startsWith("#"));
    if (first) return first.trim();
  }
  return dt.getData("text/plain").trim();
}

/** Text carried by a paste event, if any. */
export function extractPasteText(e: Pick<ClipboardEvent, "clipboardData">): string {
  return e.clipboardData?.getData("text/plain").trim() ?? "";
}

/** True when a global paste should be hijacked into capture (i.e. the user is NOT
 *  already typing into a field). */
export function shouldInterceptPaste(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return true;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return false;
  if (target.isContentEditable) return false;
  return true;
}
```

## 6. `src/lib/stores.ts` — new stores

Add alongside the existing `route` / `currentPage` writables:

```ts
// Whether the app has been configured (key + wiki folder). Gates the main UI.
export const configured = writable(false);

// Text to pre-fill the capture input with on the next visit to the capture view.
// The shell sets this on drop/paste; Home consumes and clears it.
export const capturePrefill = writable("");
```

(`"capture"` is already in the `Route` union and already rendered by `+page.svelte`.)

## 7. `src/routes/+page.svelte` — gate + capture handlers

Responsibilities:

1. **Settings load + gate.** On mount, `getSettings()`; set `configured` from
   `isConfigured(s)`; track a local `loading` flag for the initial fetch.
   - While `loading` → render a centered `<Spinner>` (no Rail).
   - Else if **not** `$configured` → render `<Onboarding />` full-bleed (no Rail).
   - Else → the normal `<Rail/>` + routed views (unchanged structure).
2. **Window capture handlers** (only active once `$configured`):
   - **Shortcut:** `keydown` where `(e.metaKey || e.ctrlKey) && e.key === "n"` →
     `preventDefault()`, `route.set("capture")`, focus handled by Home.
   - **Drop:** `dragover` (call `preventDefault` so drop fires) + `drop` →
     `text = extractDropText(e.dataTransfer)`; if non-empty: `preventDefault()`,
     `capturePrefill.set(text)`, `route.set("capture")`.
   - **Paste:** `paste` → if `shouldInterceptPaste(e.target)` and
     `text = extractPasteText(e)` is non-empty: `preventDefault()`,
     `capturePrefill.set(text)`, `route.set("capture")`.
   - Listeners are added in `onMount` and removed in `onDestroy` (or via Svelte
     `<svelte:window on:...>`, which auto-cleans — preferred).

Use `<svelte:window on:keydown on:paste on:dragover on:drop>` with guards that early-return
unless `$configured`. The drag-drop relies on DOM events for text/URI drags (Tauri's
file-drop interception only affects *file* drops, which are out of scope here).

## 8. `src/lib/components/Onboarding.svelte`

Single-screen neo-brutalist form. Local `Settings` seeded with the app defaults
(`provider:"claude"`, `model:"claude-opus-4-8"`, `embed_provider:"hash"`,
`embed_model:"nomic-embed-text"`, `ollama_url:"http://localhost:11434"`, `api_key:""`,
`wiki_path:""`).

- Two visible required fields: **API key** (`type="password"`) and **Wiki folder** (text,
  placeholder `/Users/you/wiki`). Provider/model/embedding stay at defaults (not shown —
  the user can refine them later in Settings).
- A short heading + one line explaining what the app does.
- "Get started" button: disabled unless `isConfigured(local)` is true. On click:
  `busy = true`; `await setSettings(local)`; on success `configured.set(true)`; on failure
  set an inline `error` string (e.g. an unwritable folder surfaces here). `busy = false`
  in a `finally`.
- Errors render in the existing neo-brutalist error style (`color:var(--pink)`).

Because onboarding defaults the embedder to **hash (offline)**, `setSettings` performs no
network calls during the index rebuild; it succeeds as long as the wiki folder is usable.
The Claude API key is not validated at save time (consistent with existing Settings).

## 9. `src/lib/components/Spinner.svelte`

A small reusable indeterminate indicator in the neo-brutalist vocabulary (thick border,
hard edges; a simple CSS animation is fine). Optional `label` prop rendered beside it.
Used by the shell (initial load), Home (capture + initial list), Browse, and Ask.

## 10. `src/lib/components/EmptyState.svelte`

A neo-brutalist card. Props: `title: string`, `subtext?: string`, and an optional default
slot for an action (e.g. a button). Renders centered within its pane.

Applied to:
- **Home** "Recent": when `pages.length === 0` after load → "No pages yet" /
  "Capture a link or note above to get started."
- **Browse:** when there are no pages / nothing is selected → "Nothing here yet" with a
  prompt to capture.
- **Ask:** before the first question and when there are no pages to search → a short
  prompt.
- **Graph:** replace its current bespoke empty markup with `<EmptyState>` (same copy:
  "No concepts yet — capture something from Home.").

## 11. Loading states (indeterminate)

- **Home capture:** replace the bare `"Digesting…"` button text with the form disabled
  plus a `<Spinner label="Digesting…" />`. Keep the button busy-disabled.
- **Home initial list:** show a `<Spinner>` (or skeleton) while the `onMount` `listPages()`
  is in flight, before deciding between the list and the empty state.
- **Browse:** show a `<Spinner>` while `getPageView` is loading a page.
- **Ask:** show a `<Spinner>` while `askQuestion` is in flight.

No backend progress events; all indicators are indeterminate.

## 12. Capture prefill flow

`Home.svelte` (rendered for both `home` and `capture` routes — the same component is not
remounted on a `home → capture` switch) handles focus and prefill reactively:

- **Focus on capture:** when `$route === "capture"`, focus the input
  (`bind:this` + `.focus()` after `tick()`). This serves the `Cmd/Ctrl+N` shortcut, which
  routes to capture with no prefill.
- **Prefill:** when `capturePrefill` becomes non-empty, set the local `input` to that
  value and clear the store (`capturePrefill.set("")`). Drop/paste route to capture **with**
  a prefill; the focus rule above then focuses the now-filled input.

Guard the reactive focus so it fires on the transition into `capture` (e.g. only when the
input element is bound), not on every unrelated reactivity tick.

## 13. Testing

### 13.1 Frontend (vitest) — `src/lib/capture.test.ts`

- `isConfigured`: false when key blank, false when folder blank, false when both blank
  (incl. whitespace-only), true when both present.
- `extractDropText`: returns the first non-comment line of `text/uri-list`; falls back to
  `text/plain` when no uri-list; returns `""` for a null DataTransfer / empty data.
- `extractPasteText`: returns trimmed `text/plain`; `""` when `clipboardData` is null.
- `shouldInterceptPaste`: false for INPUT/TEXTAREA/SELECT/contentEditable targets; true for
  a plain element and for `null`.

### 13.2 Existing gates

Components are verified via `npm run check` (0 errors) and `npm run build`. The full suite
(`cargo test`, `npm run test`, `npm run build`) must stay green; Rust is unchanged so its
tests are unaffected.

## 14. Out of scope (clean follow-ups)

- OS-global hotkey + tray/background summon (deferred earlier).
- Dropping/reading files (only links/text in this slice).
- Real phased capture progress via backend events.
- Folder-picker dialog for the wiki path (plain text input for now, matching Settings).
- Quick-switcher / command palette between pages (a separate M4 keyboard-nav idea).
- Onboarding API-key validation (a live test call against the provider).

## 15. Acceptance

- A fresh, unconfigured app shows the onboarding form (no Rail) and cannot reach the main
  UI until an API key + wiki folder are saved; saving opens the app.
- Every view shows a sensible empty state when it has no data, and an indeterminate loading
  indicator while fetching.
- `Cmd/Ctrl+N` focuses the capture input; dropping a link/text anywhere prefills capture;
  pasting anywhere (when not typing in a field) prefills capture.
- No Rust changes, no new commands, no new plugins/capabilities; no sensitive area touched.
- All gates green: `cargo test`, `cargo clippy --all-targets`, `cargo fmt`, `npm run check`,
  `npm run test`, `npm run build`.
