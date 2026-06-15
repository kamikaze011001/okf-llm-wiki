# OKF LLM Wiki — Design Spec

**Date:** 2026-06-16
**Status:** Approved for planning
**Owner:** sonanh

---

## 1. Summary

A **local-first desktop app** that turns sources you drop in (links, notes, later PDFs/videos) into a
personal, interlinked knowledge base, where an LLM does all the reading, summarizing, linking, and
filing — and you get a friction-free way to learn from it later.

It combines two ideas:

- **Karpathy's "LLM Wiki"** — the *workflow*: the LLM writes and maintains the wiki; the human only
  curates sources and asks questions. The wiki is a persistent, compounding artifact.
- **Open Knowledge Format (OKF)** — the *storage format*: a plain folder of Markdown files with YAML
  frontmatter, `index.md`, `log.md`, and `[[links]]`. Portable, vendor-neutral, readable without the app.

**North star:** a lazy person can learn new things with zero barrier. Capture is effortless; the payoff
is a delightful Learn surface.

---

## 2. Goals & Non-Goals

### Goals
- Zero-effort capture: drop a source → it digests in the background → notify when ready.
- LLM-written, LLM-maintained OKF pages that cross-link and compound over time.
- A friendly reading experience over raw OKF (TL;DR-first, "Connects to", preserved save-notes).
- An **Ask** surface: chat over your own curated knowledge with citations to source pages.
- **Provider-agnostic** intelligence: swap Claude / OpenAI / Ollama behind one interface.
- Local-first & private by default; data is a plain folder the user owns forever.
- Neo-brutalist UI that deliberately avoids generic "AI slop."

### Non-Goals (for v1)
- PDF and YouTube ingestion (planned next layer — see §9).
- Feed, Quiz/flashcards, and Guided Tours surfaces (planned next layers — see §9).
- Multi-user, sharing, sync, or cloud hosting.
- Mobile apps.
- Inventing or extending the OKF spec — we consume v0.1 as-is.

---

## 3. Scope: the v1 thin vertical slice

Build the **entire loop** end-to-end for the narrowest case, then widen:

> **Paste a URL or plain text → auto-digest into OKF pages → Browse the wiki → Ask questions over it.**

Everything else (more source types, more learn modes) is an additive layer that plugs into this
foundation without changing it. Rationale: prove the compounding-knowledge hypothesis fastest, on a
foundation later features reuse.

---

## 4. Architecture

Five layers inside a **Tauri shell** (Rust core + web UI). Only LLM/embedding calls leave the machine,
and only to the provider the user configures.

```
Tauri Shell  (Rust core: files, background jobs, provider calls  |  Web UI: the screens)
  │
  ① Capture          paste URL / plain text  → enqueue digest job, notify on completion
  ▼
  ② Ingest Pipeline  fetch & clean → LLM digests → write page → link/update related → append log.md
  │                  (LLM behind a swappable LLMProvider interface)
  ▼
  ③ OKF Store        folder of Markdown + YAML frontmatter; index.md, log.md, [[links]]
  ▼
  ④ Retrieval        local full-text search + embeddings index over pages
  ▼
  ⑤ Learn Surface    Browse + Ask (v1); Feed / Quiz / Tours (later)
```

**Key invariant — the OKF folder is the contract.** Capture writes to it; Learn reads from it. The two
sides are decoupled through the filesystem, so either can be rebuilt independently and external tools
(Obsidian, Claude Code) could point at the same folder.

### Component responsibilities

| Component | Does | Depends on | Interface |
|---|---|---|---|
| **Capture UI** | Accept URL/text, optional save-note, show queue/status | Job queue | "submit source" command |
| **Source Fetcher** | Fetch URL → readable text (strip nav/ads); pass-through for plain text | HTTP | `fetch(source) -> CleanText` |
| **Digest Service** | Prompt LLM to write/update OKF pages from CleanText + existing related pages | LLMProvider, OKF Store, Retrieval | `digest(CleanText, note) -> PageWrites` |
| **LLMProvider** | Abstract chat/completion + embeddings | provider SDKs/HTTP | `complete()`, `embed()` |
| **OKF Store** | Read/write/validate Markdown+frontmatter; manage index.md & log.md | filesystem | `getPage`, `writePage`, `listPages`, `appendLog` |
| **Retrieval** | Index pages; keyword + vector search | OKF Store, LLMProvider.embed | `search(query) -> PageRefs` |
| **Browse UI** | Render pages (TL;DR-first), navigate links, graph view | OKF Store | — |
| **Ask UI** | Chat; retrieve context; answer with citations; offer to file answer as page | Retrieval, LLMProvider, OKF Store | — |
| **Settings** | Choose provider/model/key, wiki folder location | config store | — |

Each unit has one purpose, communicates through a named interface, and is testable in isolation
(e.g., Digest Service tested with a fake LLMProvider; Retrieval tested against a fixture OKF folder).

---

## 5. Data model (OKF)

The wiki is a user-chosen folder. We consume OKF v0.1 conventions:

```
wiki/
├── index.md                 # navigation; declares okf_version: "0.1"
├── log.md                   # append-only chronological change history
├── concepts/<concept>.md    # one concept per file; path = identity
├── sources/<source>.md      # per-source summary pages
└── …/
```

Each page = YAML frontmatter + Markdown body.

```markdown
---
type: Concept            # required (Concept | Source | Playbook | …)
title: Vitamin D & Sleep
description: One-line summary.
tags: [sleep, supplements, health]
resource: https://youtu.be/…     # canonical source URI (optional)
timestamp: 2026-06-16T09:00:00Z
note: "check if this explains my winter insomnia"   # user's save-note (optional, our extension)
---

**TL;DR.** …

## Key points
- …

## Connects to
- [[Magnesium & Sleep]]
```

Notes:
- Unknown frontmatter keys are allowed by OKF; our `note` field is a benign extension.
- Links use Markdown/wiki-style links; the relationship graph is derived from them.
- Files are the source of truth. The embeddings/search index is a derived cache that can be rebuilt.

---

## 6. Key flows

### Capture → Digest (fully automatic)
1. User pastes a URL or text in Capture, optionally adds a one-line save-note, hits enter.
2. A digest **job** is enqueued; UI shows it as "Digesting…". User is free to do anything else.
3. Source Fetcher produces clean text.
4. Retrieval finds the most related existing pages (for context + which to update).
5. Digest Service prompts the LLMProvider to: write/replace the source/concept page(s), update
   related pages' "Connects to" + any changed claims, and produce a `log.md` entry.
6. OKF Store writes the files; Retrieval re-indexes the changed pages.
7. User gets a **notification**: "New page: Vitamin D & Sleep (updated 3 related)." Nothing was required of them.

### Ask
1. User asks a question.
2. Retrieval returns the top relevant pages.
3. LLMProvider answers grounded in those pages, with **citations** linking to the pages.
4. User can click **"Save this answer as a page"** → becomes a new Concept page so explorations compound.

### Browse
- Home (calm launcher) → recent pages + the universal capture/ask bar.
- Page view: TL;DR-first render, chips, "Connects to" navigation, preserved save-note, source link,
  per-page actions (Ask about this, Graph, Edit, Forget). Future: "Make flashcards".
- Graph view: pages as nodes, links as edges.

---

## 7. UI / Visual design

**Layout:** sidebar rail (Home · Capture · Browse · Ask · Learn[soon] · Settings) + a Home screen that
*is* a calm centered capture/ask bar with recent pages below. Lazy days live on Home; the rail gives
discoverability and room for future modes.

**Visual language — neo-brutalism, explicitly anti-"AI slop":**
- **Do:** ~3px ink/black borders; hard offset shadows (4–8px, solid, no blur); flat "paper" background;
  flat vivid accent blocks (blue / yellow / pink / green); heavy type (800–900 weight, uppercase
  headings); monospace for raw OKF/frontmatter/code; high contrast.
- **Don't:** purple/indigo gradients, glassmorphism/blur, soft mushy rounded cards, soft drop shadows,
  emoji confetti, generic SaaS aesthetics.

This is a first-class requirement, not a finishing touch.

---

## 8. Cross-cutting concerns

- **Provider abstraction:** `LLMProvider` with `complete()` and `embed()`. Adapters: Claude (default),
  OpenAI, Ollama. Provider + model + API key + wiki-folder path are set in Settings. Missing/invalid
  key surfaces a clear error, never a silent failure.
- **Background jobs:** digest runs off the UI thread; queue survives app restart; a failed job is
  retryable and visible, never lost.
- **Error handling:** fetch failures, provider errors, and malformed LLM output each degrade gracefully
  — the source is kept, the user is told, nothing corrupts the OKF folder. Writes are atomic
  (write-temp-then-rename); `log.md` is append-only.
- **Privacy:** everything is local except provider calls. The chosen provider and what it receives are
  transparent to the user. No telemetry.
- **Data ownership:** the wiki folder is plain files; deleting the app leaves a fully usable OKF wiki.
  Recommend (not require) the folder be a git repo for history/backup.
- **Performance:** Browse/Ask read from the local index and stay responsive while digests run.

---

## 9. Roadmap beyond v1

Each layer is additive and isolated:

- **Source types:** PDF drop, YouTube (transcript) → new Source Fetcher adapters only.
- **Learn modes (all reuse Retrieval ④):**
  - **Feed** — scrollable bite-sized digest of what's new / key ideas.
  - **Quiz / flashcards** — spaced-repetition cards generated per page.
  - **Guided Tours** — LLM strings pages into a short ordered lesson path.
- **Lint** — periodic health check: contradictions, stale claims, orphans, missing links (Karpathy's
  third core operation).

---

## 10. Testing strategy

- **Unit:** Source Fetcher (HTML→text fixtures), OKF Store (round-trip frontmatter, atomic writes,
  log append), Digest Service (fake LLMProvider → assert PageWrites), Retrieval (fixture folder →
  expected ranking).
- **Integration:** full capture→digest→browse→ask loop against a stub provider returning canned output.
- **Provider adapters:** contract tests each adapter satisfies the `LLMProvider` interface.
- **Manual/visual:** neo-brutalist design review of each screen against §7.

---

## 11. Open questions (resolve during planning)

- Tauri front-end framework (e.g., Svelte vs React) — pick for bundle size + neo-brutalist control.
- Embeddings: which model per provider, and a local fallback for offline search.
- Exact prompt design for digest (how aggressively to update related pages without churn).
- Folder layout convention (`concepts/` + `sources/` vs flat) — confirm against OKF sample bundles.
