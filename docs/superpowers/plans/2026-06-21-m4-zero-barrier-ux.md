# M4 Zero-Barrier UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-run onboarding (a hard gate), real empty/loading states across every view, and frictionless capture (in-app shortcut + global drop/paste of links/text) — all frontend-only.

**Architecture:** Pure, unit-tested helpers in `src/lib/capture.ts`; two new app stores; three new neo-brutalist components (`Spinner`, `EmptyState`, `Onboarding`); an onboarding gate plus window-level capture handlers in the app shell `+page.svelte`; and empty/loading wiring in Home/Browse/Ask/Graph. No Rust, no new Tauri commands/plugins, no sensitive areas touched.

**Tech Stack:** SvelteKit + Svelte 5 (legacy syntax: `on:`, `let`, `$:`, `onMount`), TypeScript, vitest (jsdom), neo-brutalist CSS tokens (`--ink --paper --blue --yellow --pink --green`, `--shadow`, `--border`) and classes (`.nb-card .nb-btn .nb-input .nb-chip`).

**Reference:** Spec at `docs/superpowers/specs/2026-06-21-m4-zero-barrier-ux-design.md`.

**Conventions:**
- Commands run from the repo root `/Users/sonanh/Documents/AIBLES/okf-llm-wiki`.
- Frontend tests: `npm run test` (vitest). Type/build gates: `npm run check` and `npm run build`. `npm run check` emits ONE pre-existing unrelated warning (`Cannot find type definition file for 'node'`) — that is expected; "0 errors" is the pass bar.
- Svelte 5 **legacy** syntax only (this codebase does not use runes). No `<style>` is stripped here (these are normal app components, not the brutalist visual companion).
- Conventional Commits. End every commit message body with exactly:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`

---

### Task 1: Pure capture helpers + tests

**Files:**
- Create: `src/lib/capture.ts`
- Test: `src/lib/capture.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `src/lib/capture.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { isConfigured, extractDropText, extractPasteText, shouldInterceptPaste } from "./capture";
import type { Settings } from "./api";

const base: Settings = {
  provider: "claude", model: "m", api_key: "", wiki_path: "",
  embed_provider: "hash", embed_model: "e", ollama_url: "u",
};

describe("isConfigured", () => {
  it("false when key blank", () => expect(isConfigured({ ...base, wiki_path: "/w" })).toBe(false));
  it("false when folder blank", () => expect(isConfigured({ ...base, api_key: "k" })).toBe(false));
  it("false when whitespace only", () => expect(isConfigured({ ...base, api_key: "  ", wiki_path: "  " })).toBe(false));
  it("true when both set", () => expect(isConfigured({ ...base, api_key: "k", wiki_path: "/w" })).toBe(true));
});

describe("extractDropText", () => {
  const dt = (m: Record<string, string>) => ({ getData: (t: string) => m[t] ?? "" });
  it("prefers first non-comment uri-list line", () =>
    expect(extractDropText(dt({ "text/uri-list": "# comment\r\nhttps://x.com\r\nhttps://y.com" }))).toBe("https://x.com"));
  it("falls back to text/plain when no uri-list", () =>
    expect(extractDropText(dt({ "text/plain": "  hello  " }))).toBe("hello"));
  it("returns empty for null", () => expect(extractDropText(null)).toBe(""));
  it("returns empty when no data", () => expect(extractDropText(dt({}))).toBe(""));
});

describe("extractPasteText", () => {
  it("returns trimmed text/plain", () =>
    expect(extractPasteText({ clipboardData: { getData: () => "  hi  " } as unknown as DataTransfer })).toBe("hi"));
  it("empty when clipboardData null", () =>
    expect(extractPasteText({ clipboardData: null })).toBe(""));
});

describe("shouldInterceptPaste", () => {
  it("false for input", () => expect(shouldInterceptPaste(document.createElement("input"))).toBe(false));
  it("false for textarea", () => expect(shouldInterceptPaste(document.createElement("textarea"))).toBe(false));
  it("false for select", () => expect(shouldInterceptPaste(document.createElement("select"))).toBe(false));
  it("true for div", () => expect(shouldInterceptPaste(document.createElement("div"))).toBe(true));
  it("true for null", () => expect(shouldInterceptPaste(null)).toBe(true));
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm run test -- capture`
Expected: FAIL — `capture.ts` does not exist / exports not found.

- [ ] **Step 3: Implement `capture.ts`**

Create `src/lib/capture.ts`:

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

/** True when a global paste should be hijacked into capture (the user is NOT typing in a field). */
export function shouldInterceptPaste(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return true;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return false;
  if (target.isContentEditable) return false;
  return true;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npm run test -- capture`
Expected: PASS — all `capture` tests green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/capture.ts src/lib/capture.test.ts
git commit -m "$(cat <<'EOF'
feat: add pure capture helpers for zero-barrier UX

isConfigured / extractDropText / extractPasteText / shouldInterceptPaste,
all unit-tested. No DOM coupling beyond instanceof checks.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: App stores for the gate and capture prefill

**Files:**
- Modify: `src/lib/stores.ts`

- [ ] **Step 1: Add the stores**

The current file is:

```ts
import { writable } from "svelte/store";
export type Route = "home" | "capture" | "browse" | "ask" | "settings" | "graph";
export const route = writable<Route>("home");
export const currentPage = writable<string | null>(null);
```

Append the two new stores so the full file reads:

```ts
import { writable } from "svelte/store";
export type Route = "home" | "capture" | "browse" | "ask" | "settings" | "graph";
export const route = writable<Route>("home");
export const currentPage = writable<string | null>(null);
// Whether the app has been configured (API key + wiki folder). Gates the main UI.
export const configured = writable(false);
// Text to pre-fill the capture input on the next visit to the capture view.
// The shell sets this on drop/paste; Home consumes and clears it.
export const capturePrefill = writable("");
```

- [ ] **Step 2: Verify it type-checks**

Run: `npm run check`
Expected: 0 errors (the single pre-existing `'node'` type warning is fine).

- [ ] **Step 3: Commit**

```bash
git add src/lib/stores.ts
git commit -m "$(cat <<'EOF'
feat: add configured + capturePrefill stores

Backing state for the onboarding gate and frictionless-capture prefill.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Shared Spinner and EmptyState components

**Files:**
- Create: `src/lib/components/Spinner.svelte`
- Create: `src/lib/components/EmptyState.svelte`

- [ ] **Step 1: Create `Spinner.svelte`**

```svelte
<script lang="ts">
  export let label = "";
</script>

<div class="nb-spin-wrap" role="status" aria-label={label || "Loading"}>
  <span class="nb-spinner" aria-hidden="true"></span>
  {#if label}<span class="nb-spin-label">{label}</span>{/if}
</div>

<style>
  .nb-spin-wrap { display: inline-flex; align-items: center; gap: 10px; }
  .nb-spin-label { font-weight: 700; }
  .nb-spinner {
    width: 18px; height: 18px;
    border: 3px solid var(--ink);
    border-top-color: var(--blue);
    display: inline-block;
    animation: nb-spin 0.8s steps(8) infinite;
  }
  @keyframes nb-spin { to { transform: rotate(360deg); } }
</style>
```

- [ ] **Step 2: Create `EmptyState.svelte`**

```svelte
<script lang="ts">
  export let title: string;
  export let subtext = "";
</script>

<div class="nb-card" style="text-align:center;max-width:420px;margin:32px auto">
  <h3 style="margin:0 0 8px 0">{title}</h3>
  {#if subtext}<p style="margin:0 0 12px 0">{subtext}</p>{/if}
  <slot />
</div>
```

- [ ] **Step 3: Verify it type-checks and builds**

Run: `npm run check`
Expected: 0 errors.

Run: `npm run build`
Expected: build succeeds (`Wrote site to "build"`).

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/Spinner.svelte src/lib/components/EmptyState.svelte
git commit -m "$(cat <<'EOF'
feat: add shared Spinner and EmptyState components

Neo-brutalist indeterminate busy indicator and empty-state card, reused
across views.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Onboarding component + hard gate

**Files:**
- Create: `src/lib/components/Onboarding.svelte`
- Modify: `src/routes/+page.svelte`

- [ ] **Step 1: Create `Onboarding.svelte`**

```svelte
<script lang="ts">
  import { setSettings, type Settings } from "$lib/api";
  import { configured } from "$lib/stores";
  import { isConfigured } from "$lib/capture";

  let s: Settings = {
    provider: "claude", model: "claude-opus-4-8", api_key: "", wiki_path: "",
    embed_provider: "hash", embed_model: "nomic-embed-text", ollama_url: "http://localhost:11434",
  };
  let busy = false;
  let error = "";

  async function start() {
    if (!isConfigured(s)) return;
    busy = true;
    error = "";
    try {
      await setSettings(s);
      configured.set(true);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<section style="min-height:100vh;display:flex;align-items:center;justify-content:center;padding:32px">
  <div class="nb-card" style="max-width:460px;width:100%;display:grid;gap:12px">
    <h1 style="margin:0">OKF Wiki</h1>
    <p style="margin:0">Paste a link or a note and it becomes a knowledge page you can browse and ask questions over. First, point it at a folder and add your Claude API key.</p>
    <label>API key<input class="nb-input" type="password" bind:value={s.api_key} placeholder="sk-ant-…" /></label>
    <label>Wiki folder<input class="nb-input" bind:value={s.wiki_path} placeholder="/Users/you/wiki" /></label>
    <button class="nb-btn accent" on:click={start} disabled={busy || !isConfigured(s)}>{busy ? "Setting up…" : "Get started"}</button>
    {#if error}<p style="color:var(--pink);font-weight:700;margin:0">⚠ {error}</p>{/if}
  </div>
</section>
```

- [ ] **Step 2: Add the gate to `+page.svelte`**

Replace the entire contents of `src/routes/+page.svelte` with:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import Rail from "$lib/components/Rail.svelte";
  import Home from "$lib/components/Home.svelte";
  import Browse from "$lib/components/Browse.svelte";
  import Ask from "$lib/components/Ask.svelte";
  import Settings from "$lib/components/Settings.svelte";
  import Graph from "$lib/components/Graph.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import Spinner from "$lib/components/Spinner.svelte";
  import { route, configured } from "$lib/stores";
  import { getSettings } from "$lib/api";
  import { isConfigured } from "$lib/capture";

  let loading = true;
  onMount(async () => {
    try {
      const s = await getSettings();
      configured.set(isConfigured(s));
    } finally {
      loading = false;
    }
  });
</script>

{#if loading}
  <main style="display:flex;align-items:center;justify-content:center;height:100vh">
    <Spinner label="Loading…" />
  </main>
{:else if !$configured}
  <Onboarding />
{:else}
  <main style="display:flex">
    <Rail />
    <div style="flex:1">
      {#if $route==="home"}<Home />{/if}
      {#if $route==="capture"}<Home />{/if}
      {#if $route==="browse"}<Browse />{/if}
      {#if $route==="ask"}<Ask />{/if}
      {#if $route==="settings"}<Settings />{/if}
      {#if $route==="graph"}<Graph />{/if}
    </div>
  </main>
{/if}
```

- [ ] **Step 3: Verify it type-checks and builds**

Run: `npm run check`
Expected: 0 errors.

Run: `npm run build`
Expected: build succeeds.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/Onboarding.svelte src/routes/+page.svelte
git commit -m "$(cat <<'EOF'
feat: add first-run onboarding hard gate

Block the main UI behind a single-screen setup form until an API key and
wiki folder are saved; show a loading spinner during the initial settings
read.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Frictionless capture — window handlers + Home prefill/focus

**Files:**
- Modify: `src/routes/+page.svelte`
- Modify: `src/lib/components/Home.svelte`

- [ ] **Step 1: Add window capture handlers to `+page.svelte`**

In the `<script>`, extend the store/helper imports and add the handler functions. The import lines become:

```ts
  import { route, configured, capturePrefill } from "$lib/stores";
  import { getSettings } from "$lib/api";
  import { isConfigured, extractDropText, extractPasteText, shouldInterceptPaste } from "$lib/capture";
```

Add these functions after the `onMount(...)` block (still inside `<script>`):

```ts
  function onKeydown(e: KeyboardEvent) {
    if (!$configured) return;
    if ((e.metaKey || e.ctrlKey) && (e.key === "n" || e.key === "N")) {
      e.preventDefault();
      route.set("capture");
    }
  }
  function onDragOver(e: DragEvent) {
    if (!$configured) return;
    e.preventDefault(); // allow the drop event to fire
  }
  function onDrop(e: DragEvent) {
    if (!$configured) return;
    const text = extractDropText(e.dataTransfer);
    if (!text) return;
    e.preventDefault();
    capturePrefill.set(text);
    route.set("capture");
  }
  function onPaste(e: ClipboardEvent) {
    if (!$configured) return;
    if (!shouldInterceptPaste(e.target)) return;
    const text = extractPasteText(e);
    if (!text) return;
    e.preventDefault();
    capturePrefill.set(text);
    route.set("capture");
  }
```

Add the `<svelte:window>` element at the very top of the markup (immediately after `</script>`, before the `{#if loading}` block):

```svelte
<svelte:window on:keydown={onKeydown} on:dragover={onDragOver} on:drop={onDrop} on:paste={onPaste} />
```

- [ ] **Step 2: Wire prefill + focus into `Home.svelte`**

The current `Home.svelte` script header is:

```ts
  import { onMount } from "svelte";
  import { listPages, submitSource, type PageDto } from "$lib/api";
  import { route, currentPage } from "$lib/stores";
  let input = ""; let note = ""; let busy = false; let pages: PageDto[] = [];
  onMount(async () => { pages = await listPages(); });
```

Replace it with (adds `tick`, the `capturePrefill` import, the input element ref, and the reactive focus/prefill):

```ts
  import { onMount, tick } from "svelte";
  import { listPages, submitSource, type PageDto } from "$lib/api";
  import { route, currentPage, capturePrefill } from "$lib/stores";
  let input = ""; let note = ""; let busy = false; let pages: PageDto[] = [];
  let inputEl: HTMLInputElement;
  onMount(async () => { pages = await listPages(); });

  // Apply a pending prefill (from a drop/paste), then clear it.
  $: applyPrefill($capturePrefill);
  function applyPrefill(p: string) {
    if (p) { input = p; capturePrefill.set(""); }
  }
  // Focus the capture input when the capture route becomes active.
  $: if ($route === "capture" && inputEl) focusInput();
  async function focusInput() { await tick(); inputEl?.focus(); }
```

Bind the element ref on the first capture input. The current line:

```svelte
    <input class="nb-input" placeholder="Paste a link or write a note…" bind:value={input} />
```

becomes:

```svelte
    <input class="nb-input" bind:this={inputEl} placeholder="Paste a link or write a note…" bind:value={input} />
```

- [ ] **Step 3: Verify it type-checks and builds**

Run: `npm run check`
Expected: 0 errors.

Run: `npm run build`
Expected: build succeeds.

- [ ] **Step 4: Commit**

```bash
git add src/routes/+page.svelte src/lib/components/Home.svelte
git commit -m "$(cat <<'EOF'
feat: frictionless capture via shortcut, drop, and paste

Cmd/Ctrl+N focuses capture; dropping or pasting a link/text anywhere in
the window routes to capture and prefills the input (when not typing in a
field). Window handlers guarded by the configured gate.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Empty and loading states across views

**Files:**
- Modify: `src/lib/components/Home.svelte`
- Modify: `src/lib/components/Browse.svelte`
- Modify: `src/lib/components/Ask.svelte`
- Modify: `src/lib/components/Graph.svelte`

- [ ] **Step 1: Home — list loading, empty Recent, capture spinner**

In `Home.svelte`, add a `loadingList` flag and import the shared components. Update the script's import block and `onMount`:

```ts
  import { onMount, tick } from "svelte";
  import { listPages, submitSource, type PageDto } from "$lib/api";
  import { route, currentPage, capturePrefill } from "$lib/stores";
  import Spinner from "$lib/components/Spinner.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  let input = ""; let note = ""; let busy = false; let pages: PageDto[] = [];
  let inputEl: HTMLInputElement;
  let loadingList = true;
  onMount(async () => { try { pages = await listPages(); } finally { loadingList = false; } });
```

Replace the capture button line:

```svelte
    <button class="nb-btn accent" style="margin-top:12px" on:click={go} disabled={busy}>{busy ? "Digesting…" : "Capture"}</button>
```

with a button plus an inline spinner while busy:

```svelte
    <button class="nb-btn accent" style="margin-top:12px" on:click={go} disabled={busy}>Capture</button>
    {#if busy}<div style="margin-top:12px"><Spinner label="Digesting…" /></div>{/if}
```

Replace the Recent list block:

```svelte
  <h3>Recent</h3>
  {#each pages as p}
    <button class="nb-card" style="display:block;width:100%;text-align:left;margin-bottom:8px;cursor:pointer"
      on:click={() => { currentPage.set(p.path); route.set("browse"); }}>
      <strong>{p.title}</strong>
      <div>{#each p.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
    </button>
  {/each}
```

with:

```svelte
  <h3>Recent</h3>
  {#if loadingList}
    <Spinner label="Loading…" />
  {:else if pages.length === 0}
    <EmptyState title="No pages yet" subtext="Capture a link or note above to get started." />
  {:else}
    {#each pages as p}
      <button class="nb-card" style="display:block;width:100%;text-align:left;margin-bottom:8px;cursor:pointer"
        on:click={() => { currentPage.set(p.path); route.set("browse"); }}>
        <strong>{p.title}</strong>
        <div>{#each p.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
      </button>
    {/each}
  {/if}
```

- [ ] **Step 2: Browse — loading view + empty state**

In `Browse.svelte`, import the shared components and track a `loadingView` flag. Update the import block (top of `<script>`):

```ts
  import { onMount } from "svelte";
  import { listPages, getPageView, updatePage, deletePage, createPage, type PageDto, type PageView } from "$lib/api";
  import { currentPage } from "$lib/stores";
  import Spinner from "$lib/components/Spinner.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
```

Set `loadingView` around `getPageView` inside `loadFor`. The current `loadFor` body is:

```ts
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    const enterEdit = pendingEdit;
    pendingEdit = false;
    try {
      view = await getPageView(path);
      mode = "view";
      confirmingDelete = false;
      deleteError = "";
      createError = "";
      if (enterEdit) startEdit();
    } finally {
      creating = false;
    }
  }
```

Replace it with (adds a `loadingView` flag and a declaration above it):

```ts
  let loadingView = false;
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    const enterEdit = pendingEdit;
    pendingEdit = false;
    loadingView = true;
    try {
      view = await getPageView(path);
      mode = "view";
      confirmingDelete = false;
      deleteError = "";
      createError = "";
      if (enterEdit) startEdit();
    } finally {
      creating = false;
      loadingView = false;
    }
  }
```

Replace the final empty branch of the markup. The current tail is:

```svelte
  {:else}
    <p>No pages yet — capture something from Home.</p>
  {/if}
</section>
```

becomes:

```svelte
  {:else if loadingView}
    <Spinner label="Loading…" />
  {:else}
    <EmptyState title="Nothing here yet" subtext="Capture something from Home to start browsing." />
  {/if}
</section>
```

- [ ] **Step 3: Ask — loading + empty prompt**

Replace the entire contents of `src/lib/components/Ask.svelte` with:

```svelte
<script lang="ts">
  import { askQuestion, type AnswerDto } from "$lib/api";
  import Spinner from "$lib/components/Spinner.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  let q = ""; let busy = false; let answer: AnswerDto | undefined;
  async function send(){ if(!q.trim()) return; busy = true; try { answer = await askQuestion(q); } finally { busy = false; } }
</script>
<section style="padding:32px;max-width:720px;margin:0 auto">
  <h1>Ask your wiki</h1>
  <div class="nb-card">
    <input class="nb-input" placeholder="Ask anything from your knowledge…" bind:value={q} on:keydown={(e)=> e.key==="Enter" && send()} />
    <button class="nb-btn accent" style="margin-top:12px" on:click={send} disabled={busy}>Ask</button>
    {#if busy}<div style="margin-top:12px"><Spinner label="Thinking…" /></div>{/if}
  </div>
  {#if answer}
    <article class="nb-card" style="margin-top:16px;white-space:pre-wrap">{answer.text}</article>
    <h3 style="margin-top:12px">Sources</h3>
    {#each answer.citations as c}<span class="nb-chip">{c}</span>{/each}
  {:else if !busy}
    <EmptyState title="Ask your knowledge base" subtext="Answers are grounded in the pages you've captured." />
  {/if}
</section>
```

- [ ] **Step 4: Graph — standardize the empty state**

In `Graph.svelte`, import `EmptyState` at the end of the existing imports in `<script>` (the file already imports from `$lib/api` and svelte). Add:

```ts
  import EmptyState from "$lib/components/EmptyState.svelte";
```

Replace the empty branch. The current line (around the `{:else if nodes.length === 0}` branch) is:

```svelte
    <div class="nb-card" style="margin:32px">No concepts yet — capture something from Home.</div>
```

becomes:

```svelte
    <EmptyState title="No concepts yet" subtext="Capture something from Home." />
```

- [ ] **Step 5: Verify the full frontend suite is green**

Run: `npm run check`
Expected: 0 errors.

Run: `npm run test`
Expected: all tests pass (existing 13 + the new `capture` tests).

Run: `npm run build`
Expected: build succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/Home.svelte src/lib/components/Browse.svelte src/lib/components/Ask.svelte src/lib/components/Graph.svelte
git commit -m "$(cat <<'EOF'
feat: empty and loading states across all views

Home (list loading, empty Recent, capture spinner), Browse (loading view +
empty state), Ask (thinking spinner + empty prompt), and Graph (shared
EmptyState) now show indeterminate loading and real empty states.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Final verification (after all tasks)

Run the full gate suite from the repo root:

```bash
npm run check   # 0 errors (one pre-existing 'node' type warning is OK)
npm run test    # all vitest green
npm run build   # build succeeds
```

Rust is untouched, so `cargo test` / `cargo clippy` are unaffected — but a quick `cd src-tauri && $HOME/.cargo/bin/cargo test` confirms nothing regressed.

## Acceptance checklist (from spec §15)

- [ ] Fresh/unconfigured app shows onboarding (no Rail); saving key + folder opens the app.
- [ ] Every view shows a sensible empty state and an indeterminate loading indicator.
- [ ] `Cmd/Ctrl+N` focuses capture; dropping a link/text prefills capture; pasting (when not in a field) prefills capture.
- [ ] No Rust changes, no new commands/plugins/capabilities, no sensitive area touched.
- [ ] All gates green.
