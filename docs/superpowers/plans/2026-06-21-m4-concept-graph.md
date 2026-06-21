# M4 Concept-Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a standalone **Graph** route that renders the whole wiki as a force-directed node-link diagram of existing concept pages and their `[[link]]` edges, with click-to-open, pan/zoom, hover-highlight, and node-drag.

**Architecture:** The link graph is already kept fresh in `AppState.links`. The backend adds a read-only `graph_data()` accessor on `LinkGraph` and a thin `get_graph` Tauri command. The frontend adds a `d3-force` simulation (layout math only) feeding a hand-rolled neo-brutalist SVG renderer, with pan/zoom/drag done via SVG transforms and pointer events (no `d3-zoom`/`d3-drag`).

**Tech Stack:** Rust + Tauri 2 (core/links + commands), SvelteKit + Svelte 5 (legacy syntax, matching existing components), `d3-force`, custom SVG.

**Spec:** `docs/superpowers/specs/2026-06-21-m4-concept-graph-design.md`

**Project notes for the implementer:**
- Run `cargo` as `$HOME/.cargo/bin/cargo` (not on PATH) from inside `src-tauri/`.
- Shell `cd` does not persist between commands — use compound `&&` (e.g. `cd src-tauri && $HOME/.cargo/bin/cargo test`).
- `core/` must stay Tauri-free (plain Rust only).
- This slice touches **non-sensitive** areas only (`links.rs`, `commands.rs`, `lib.rs`, frontend) — no security review gate.
- Conventional Commits; end every commit message with exactly:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
- Components use **legacy Svelte syntax** (`on:click`, `let`, `$:`, `onMount`) — match that, do not use runes.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src-tauri/src/core/links.rs` | `GraphNode`/`GraphEdge`/`GraphData` + `LinkGraph::graph_data()` | Modify |
| `src-tauri/src/commands.rs` | `GraphDto` + `get_graph` command | Modify |
| `src-tauri/src/lib.rs` | Register `get_graph` in `invoke_handler` | Modify |
| `src-tauri/tests/commands_integration.rs` | Cross-page graph_data integration test | Modify |
| `package.json` | `d3-force` dep + `@types/d3-force` devDep | Modify |
| `src/lib/api.ts` | `getGraph()` + `GraphNode`/`GraphEdge`/`GraphData` types | Modify |
| `src/lib/api.test.ts` | `getGraph` wrapper test | Modify |
| `src/lib/graph-model.ts` | Pure `buildGraphModel()` + `nodeRadius()` + `SimNode`/`SimLink` | Create |
| `src/lib/graph-model.test.ts` | Unit tests for the model helper | Create |
| `src/lib/components/Graph.svelte` | The graph view (d3 sim + SVG + interactions) | Create |
| `src/lib/stores.ts` | Add `"graph"` to `Route` | Modify |
| `src/lib/components/Rail.svelte` | Add Graph nav button | Modify |
| `src/routes/+page.svelte` | Render `<Graph/>` for the graph route | Modify |

---

## Task 1: Backend — `graph_data()` on `LinkGraph`

**Files:**
- Modify: `src-tauri/src/core/links.rs` (add structs after `ConceptRef` ~line 81; add `graph_data` inside `impl LinkGraph`; add tests in the `tests` module)

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the existing `#[cfg(test)] mod tests { ... }` block in `src-tauri/src/core/links.rs` (the `tmp()` and `page()` helpers already exist there):

```rust
    #[test]
    fn graph_data_nodes_edges_and_degree() {
        let store = OkfStore::new(tmp());
        store
            .write_page(&page(
                "concepts/alpha.md",
                "Alpha",
                "links [[Beta]] and [[Gamma]], and itself [[Alpha]].",
            ))
            .unwrap();
        store
            .write_page(&page("concepts/beta.md", "Beta", "links back to [[Alpha]]."))
            .unwrap();
        store
            .write_page(&page("concepts/gamma.md", "Gamma", "no links here."))
            .unwrap();
        store
            .write_page(&page(
                "concepts/orphan.md",
                "Orphan",
                "alone, mentions [[Ghost]].",
            ))
            .unwrap();
        let data = build_link_graph(&store).unwrap().graph_data();

        // All four existing pages are nodes (orphan included), sorted by path.
        let node_paths: Vec<&str> = data.nodes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            node_paths,
            vec![
                "concepts/alpha.md",
                "concepts/beta.md",
                "concepts/gamma.md",
                "concepts/orphan.md",
            ]
        );

        // Edges: Alpha-Beta is mutual -> one edge; Alpha-Gamma; no edge to unresolved
        // Ghost; no self-loop from Alpha's own [[Alpha]].
        assert_eq!(
            data.edges,
            vec![
                GraphEdge {
                    source: "concepts/alpha.md".into(),
                    target: "concepts/beta.md".into()
                },
                GraphEdge {
                    source: "concepts/alpha.md".into(),
                    target: "concepts/gamma.md".into()
                },
            ]
        );

        // Degrees from the deduped edge set.
        let deg = |p: &str| data.nodes.iter().find(|n| n.path == p).unwrap().degree;
        assert_eq!(deg("concepts/alpha.md"), 2);
        assert_eq!(deg("concepts/beta.md"), 1);
        assert_eq!(deg("concepts/gamma.md"), 1);
        assert_eq!(deg("concepts/orphan.md"), 0);
    }

    #[test]
    fn graph_data_empty_store_is_empty() {
        let store = OkfStore::new(tmp());
        let data = build_link_graph(&store).unwrap().graph_data();
        assert!(data.nodes.is_empty());
        assert!(data.edges.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib graph_data`
Expected: FAIL to compile — `GraphEdge` and `graph_data` are not defined.

- [ ] **Step 3: Add the structs**

In `src-tauri/src/core/links.rs`, immediately after the `ConceptRef` struct (around line 81, before `pub struct LinkGraph`), add:

```rust
/// A concept page as a graph node. `degree` counts undirected `[[link]]` edges touching it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub path: String,
    pub title: String,
    pub degree: usize,
}

/// An undirected `[[link]]` edge between two existing pages. `source`/`target` are page
/// paths ordered lexically — the direction is not semantically meaningful.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

/// The whole-wiki concept graph: every existing page plus its deduped link edges.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}
```

- [ ] **Step 4: Implement `graph_data`**

In `src-tauri/src/core/links.rs`, inside `impl LinkGraph { ... }` (after the `backlinks` method, before the closing brace ~line 146), add:

```rust
    /// The whole-wiki concept graph. Nodes are all existing pages (orphans included,
    /// degree 0). Edges are `[[link]]`s between existing pages: undirected, deduplicated
    /// (a mutual A<->B link is one edge), self-links and unresolved targets excluded.
    /// Nodes and edges are sorted by path for deterministic output.
    pub fn graph_data(&self) -> GraphData {
        // Collect undirected, deduped edges keyed by the lexically-ordered pair of paths.
        let mut pairs: HashSet<(String, String)> = HashSet::new();
        for (target_slug, sources) in &self.backlinks {
            let Some(target_path) = self.slug_to_path.get(target_slug) else {
                continue; // unresolved target
            };
            for source_path in sources {
                if source_path == target_path {
                    continue; // self-link (defensive)
                }
                let pair = if source_path <= target_path {
                    (source_path.clone(), target_path.clone())
                } else {
                    (target_path.clone(), source_path.clone())
                };
                pairs.insert(pair);
            }
        }

        // Degree per page from the deduped pairs.
        let mut degree: HashMap<String, usize> = HashMap::new();
        for (a, b) in &pairs {
            *degree.entry(a.clone()).or_default() += 1;
            *degree.entry(b.clone()).or_default() += 1;
        }

        let mut nodes: Vec<GraphNode> = self
            .slug_to_path
            .iter()
            .map(|(slug, path)| GraphNode {
                path: path.clone(),
                title: self
                    .slug_to_title
                    .get(slug)
                    .cloned()
                    .unwrap_or_else(|| slug.clone()),
                degree: degree.get(path).copied().unwrap_or(0),
            })
            .collect();
        nodes.sort_by(|a, b| a.path.cmp(&b.path));

        let mut edges: Vec<GraphEdge> = pairs
            .into_iter()
            .map(|(source, target)| GraphEdge { source, target })
            .collect();
        edges.sort_by(|a, b| {
            (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str()))
        });

        GraphData { nodes, edges }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --lib graph_data`
Expected: PASS (both tests).

- [ ] **Step 6: Lint + format**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt`
Expected: no warnings; fmt makes no further changes.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/core/links.rs
git commit -m "$(printf 'feat: add LinkGraph::graph_data for the concept graph\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

## Task 2: Backend — `get_graph` command + registration + integration test

**Files:**
- Modify: `src-tauri/src/commands.rs` (add DTOs near the other DTOs ~lines 19-53; add the command)
- Modify: `src-tauri/src/lib.rs:34-45` (register in `invoke_handler`)
- Modify: `src-tauri/tests/commands_integration.rs` (import `GraphEdge`; add a test)

- [ ] **Step 1: Write the failing integration test**

In `src-tauri/tests/commands_integration.rs`, change the `links` import (currently lines 5-7) to add `GraphEdge`:

```rust
use okf_llm_wiki_lib::core::links::{
    build_link_graph, concept_refs, segment_body, BacklinkRef, GraphEdge, Segment,
};
```

Then append this test at the end of the file:

```rust
#[tokio::test]
async fn graph_data_reflects_links_across_pages() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Alpha exists.
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

    // Beta links to Alpha (Alpha is in the allow-list, so the link survives validation).
    let existing = concept_refs(&store).unwrap();
    let b = digest(
        &FakeProvider {
            reply:
                r#"{"title":"Beta","description":"d","tags":[],"body":"Beta builds on [[Alpha]]."}"#
                    .into(),
        },
        "src b",
        None,
        None,
        &existing,
    )
    .await
    .unwrap();
    store.write_page(&b.page).unwrap();

    let data = build_link_graph(&store).unwrap().graph_data();
    let node_paths: Vec<&str> = data.nodes.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(node_paths, vec!["concepts/alpha.md", "concepts/beta.md"]);
    assert_eq!(
        data.edges,
        vec![GraphEdge {
            source: "concepts/alpha.md".into(),
            target: "concepts/beta.md".into()
        }]
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test --test commands_integration graph_data_reflects_links_across_pages`
Expected: FAIL to compile — `GraphEdge` import unresolved until Task 1 is in (it is) and the test is new; it should compile and PASS once Task 1's `graph_data` exists. If Task 1 is already committed, this test will actually PASS immediately — that is acceptable (it documents the command's data source). Continue to add the command itself in the next steps.

- [ ] **Step 3: Add the DTOs**

In `src-tauri/src/commands.rs`, after the `PageViewDto` struct (ends ~line 53), add:

```rust
#[derive(Serialize)]
pub struct GraphNodeDto {
    pub path: String,
    pub title: String,
    pub degree: usize,
}

#[derive(Serialize)]
pub struct GraphEdgeDto {
    pub source: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct GraphDto {
    pub nodes: Vec<GraphNodeDto>,
    pub edges: Vec<GraphEdgeDto>,
}
```

- [ ] **Step 4: Add the command**

In `src-tauri/src/commands.rs`, after the `get_page_view` command (ends ~line 178), add:

```rust
/// Return the whole-wiki concept graph (existing pages as nodes, `[[link]]` edges).
/// Reads the in-memory link graph only — no file IO, no `.await`, so no `MutexGuard`
/// is held across an await point.
#[tauri::command]
pub fn get_graph(state: State<AppState>) -> Result<GraphDto, String> {
    let graph = state.links.lock().unwrap();
    let data = graph.graph_data();
    Ok(GraphDto {
        nodes: data
            .nodes
            .into_iter()
            .map(|n| GraphNodeDto {
                path: n.path,
                title: n.title,
                degree: n.degree,
            })
            .collect(),
        edges: data
            .edges
            .into_iter()
            .map(|e| GraphEdgeDto {
                source: e.source,
                target: e.target,
            })
            .collect(),
    })
}
```

- [ ] **Step 5: Register the command**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler!` list (lines 34-45), add `get_graph` after `create_page`. Change:

```rust
            commands::delete_page,
            commands::create_page
        ])
```
to:
```rust
            commands::delete_page,
            commands::create_page,
            commands::get_graph
        ])
```

- [ ] **Step 6: Build, test, lint, format**

Run: `cd src-tauri && $HOME/.cargo/bin/cargo test && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt`
Expected: all tests pass (incl. the new integration test); clippy clean; fmt no changes.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/tests/commands_integration.rs
git commit -m "$(printf 'feat: add get_graph command for the concept graph\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

## Task 3: Frontend — dependency + API client

**Files:**
- Modify: `package.json` (via npm)
- Modify: `src/lib/api.ts` (append types + `getGraph`)
- Modify: `src/lib/api.test.ts` (import `getGraph`; add a test)

- [ ] **Step 1: Install d3-force**

Run: `npm install d3-force@^3 && npm install -D @types/d3-force@^3`
Expected: `package.json` gains `"d3-force": "^3..."` under dependencies and `"@types/d3-force": "^3..."` under devDependencies; `package-lock.json` updated.

- [ ] **Step 2: Write the failing API test**

In `src/lib/api.test.ts`, add `getGraph` to the import on line 3:

```ts
import { listPages, submitSource, setSettings, getPageView, reindex, updatePage, deletePage, createPage, getGraph } from "./api";
```

Then add this test inside the `describe("api", ...)` block (before its closing `});`):

```ts
  it("getGraph invokes the get_graph command", async () => {
    await getGraph();
    expect(invoke).toHaveBeenCalledWith("get_graph");
  });
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `npm run test`
Expected: FAIL — `getGraph` is not exported from `./api`.

- [ ] **Step 4: Add the API client**

In `src/lib/api.ts`, after the `PageView` interface (line 8), add:

```ts
export interface GraphNode { path: string; title: string; degree: number; }
export interface GraphEdge { source: string; target: string; }
export interface GraphData { nodes: GraphNode[]; edges: GraphEdge[]; }
```

And at the end of the file (after `createPage`), add:

```ts
export const getGraph = () => invoke<GraphData>("get_graph");
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `npm run test`
Expected: PASS (all api tests, including the new one).

- [ ] **Step 6: Commit**

```bash
git add package.json package-lock.json src/lib/api.ts src/lib/api.test.ts
git commit -m "$(printf 'feat: add getGraph API client and d3-force dependency\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

## Task 4: Frontend — pure `buildGraphModel` helper

**Files:**
- Create: `src/lib/graph-model.ts`
- Create: `src/lib/graph-model.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `src/lib/graph-model.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { buildGraphModel, nodeRadius } from "./graph-model";
import type { GraphData } from "./api";

describe("graph-model", () => {
  it("maps nodes to sim nodes with radius growing with degree", () => {
    const data: GraphData = {
      nodes: [
        { path: "concepts/a.md", title: "A", degree: 0 },
        { path: "concepts/b.md", title: "B", degree: 5 },
      ],
      edges: [{ source: "concepts/a.md", target: "concepts/b.md" }],
    };
    const model = buildGraphModel(data);
    expect(model.nodes.map((n) => n.path)).toEqual(["concepts/a.md", "concepts/b.md"]);
    expect(model.nodes[1].r).toBeGreaterThan(model.nodes[0].r);
    expect(model.nodes[0].title).toBe("A");
  });

  it("maps edges to links by path", () => {
    const data: GraphData = {
      nodes: [
        { path: "concepts/a.md", title: "A", degree: 1 },
        { path: "concepts/b.md", title: "B", degree: 1 },
      ],
      edges: [{ source: "concepts/a.md", target: "concepts/b.md" }],
    };
    const model = buildGraphModel(data);
    expect(model.links).toEqual([{ source: "concepts/a.md", target: "concepts/b.md" }]);
  });

  it("returns an empty model for empty data", () => {
    const model = buildGraphModel({ nodes: [], edges: [] });
    expect(model.nodes).toEqual([]);
    expect(model.links).toEqual([]);
  });

  it("nodeRadius clamps at the maximum", () => {
    expect(nodeRadius(0)).toBeLessThan(nodeRadius(100));
    expect(nodeRadius(100)).toBe(nodeRadius(1000));
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm run test`
Expected: FAIL — cannot resolve `./graph-model`.

- [ ] **Step 3: Implement the helper**

Create `src/lib/graph-model.ts`:

```ts
import type { SimulationNodeDatum, SimulationLinkDatum } from "d3-force";
import type { GraphData } from "./api";

/** A graph node positioned by the d3-force simulation. x/y/fx/fy are managed by d3. */
export interface SimNode extends SimulationNodeDatum {
  path: string;
  title: string;
  degree: number;
  r: number;
}

/** A graph edge. d3-force resolves the string `path` endpoints to SimNode objects in place. */
export type SimLink = SimulationLinkDatum<SimNode>;

export interface GraphModel {
  nodes: SimNode[];
  links: SimLink[];
}

const BASE_R = 14;
const R_PER_DEGREE = 3;
const MAX_R = 34;

/** Node radius grows with degree, clamped so hubs do not dominate the canvas. */
export function nodeRadius(degree: number): number {
  return Math.min(BASE_R + degree * R_PER_DEGREE, MAX_R);
}

/** Shape backend GraphData into d3-force simulation inputs. Pure — no positions assigned
 *  (d3 initializes x/y); links reference nodes by `path` for forceLink's id accessor. */
export function buildGraphModel(data: GraphData): GraphModel {
  const nodes: SimNode[] = data.nodes.map((n) => ({
    path: n.path,
    title: n.title,
    degree: n.degree,
    r: nodeRadius(n.degree),
  }));
  const links: SimLink[] = data.edges.map((e) => ({ source: e.source, target: e.target }));
  return { nodes, links };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npm run test`
Expected: PASS (all four graph-model tests).

- [ ] **Step 5: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 6: Commit**

```bash
git add src/lib/graph-model.ts src/lib/graph-model.test.ts
git commit -m "$(printf 'feat: add buildGraphModel helper for the concept graph\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

## Task 5: Frontend — `Graph.svelte` component + route wiring

**Files:**
- Create: `src/lib/components/Graph.svelte`
- Modify: `src/lib/stores.ts:2` (add `"graph"` to `Route`)
- Modify: `src/lib/components/Rail.svelte:3-6` (add Graph nav item)
- Modify: `src/routes/+page.svelte` (import + render `<Graph/>`)

This is a UI integration task with no unit test (rendering is not unit-tested per the spec). Verification is via `npm run check` (0 errors), `npm run test` (existing tests still pass), and `npm run build` (succeeds).

- [ ] **Step 1: Add the route**

In `src/lib/stores.ts`, change line 2 to:

```ts
export type Route = "home" | "capture" | "browse" | "ask" | "settings" | "graph";
```

- [ ] **Step 2: Add the Rail nav button**

In `src/lib/components/Rail.svelte`, change the `items` array (lines 3-6) to insert Graph between Browse and Ask:

```ts
  const items: {id: Route; label: string}[] = [
    {id:"home",label:"Home"},{id:"capture",label:"＋ Capture"},
    {id:"browse",label:"Browse"},{id:"graph",label:"Graph"},
    {id:"ask",label:"Ask"},{id:"settings",label:"⚙ Settings"},
  ];
```

- [ ] **Step 3: Create the component**

Create `src/lib/components/Graph.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    forceSimulation,
    forceManyBody,
    forceLink,
    forceCenter,
    forceCollide,
    type Simulation,
  } from "d3-force";
  import { getGraph } from "$lib/api";
  import { buildGraphModel, type SimNode, type SimLink } from "$lib/graph-model";
  import { route, currentPage } from "$lib/stores";

  let nodes: SimNode[] = [];
  let links: SimLink[] = [];
  let loading = true;
  let error = "";
  let sim: Simulation<SimNode, SimLink> | undefined;
  let svgEl: SVGSVGElement;

  const WIDTH = 900;
  const HEIGHT = 640;

  // pan/zoom transform applied to the <g>
  let tx = 0;
  let ty = 0;
  let scale = 1;

  // hover highlight
  let hovered: string | null = null;
  let neighbors = new Set<string>();

  onMount(async () => {
    try {
      const model = buildGraphModel(await getGraph());
      nodes = model.nodes;
      links = model.links;
      if (nodes.length > 0) startSim();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  onDestroy(() => sim?.stop());

  function startSim() {
    sim = forceSimulation<SimNode>(nodes)
      .force("charge", forceManyBody().strength(-320))
      .force(
        "link",
        forceLink<SimNode, SimLink>(links)
          .id((d) => d.path)
          .distance(96),
      )
      .force("center", forceCenter(WIDTH / 2, HEIGHT / 2))
      .force("collide", forceCollide<SimNode>().radius((d) => d.r + 8))
      .on("tick", () => {
        // reassign to trigger Svelte reactivity
        nodes = nodes;
        links = links;
      });
  }

  // After forceLink runs, l.source/l.target are SimNode objects; before, they are path strings.
  function asNode(end: SimLink["source"]): SimNode | null {
    return typeof end === "object" ? (end as SimNode) : null;
  }

  function computeNeighbors(path: string): Set<string> {
    const s = new Set<string>();
    for (const l of links) {
      const sn = asNode(l.source);
      const tn = asNode(l.target);
      if (!sn || !tn) continue;
      if (sn.path === path) s.add(tn.path);
      if (tn.path === path) s.add(sn.path);
    }
    return s;
  }

  function enterNode(n: SimNode) {
    hovered = n.path;
    neighbors = computeNeighbors(n.path);
  }
  function leaveNode() {
    hovered = null;
    neighbors = new Set();
  }
  function dimmedNode(path: string): boolean {
    return hovered !== null && path !== hovered && !neighbors.has(path);
  }
  function fillNode(path: string): string {
    return path === hovered || neighbors.has(path) ? "#2563ff" : "#fff";
  }
  function dimmedLink(l: SimLink): boolean {
    if (hovered === null) return false;
    const sn = asNode(l.source);
    const tn = asNode(l.target);
    if (!sn || !tn) return false;
    return sn.path !== hovered && tn.path !== hovered;
  }

  function openNode(n: SimNode) {
    currentPage.set(n.path);
    route.set("browse");
  }

  // ---- pan & zoom ----
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
    const next = Math.min(Math.max(scale * factor, 0.2), 4);
    const rect = svgEl.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    tx = mx - (mx - tx) * (next / scale);
    ty = my - (my - ty) * (next / scale);
    scale = next;
  }

  let panning = false;
  let panOX = 0;
  let panOY = 0;
  function canvasDown(e: PointerEvent) {
    if (dragging) return;
    panning = true;
    panOX = e.clientX - tx;
    panOY = e.clientY - ty;
    svgEl.setPointerCapture(e.pointerId);
  }
  function canvasMove(e: PointerEvent) {
    if (dragging) {
      dragMove(e);
      return;
    }
    if (!panning) return;
    tx = e.clientX - panOX;
    ty = e.clientY - panOY;
  }
  function canvasUp() {
    panning = false;
    if (dragging) dragEnd();
  }

  // ---- node drag (distinguished from click by movement threshold) ----
  let dragging: SimNode | null = null;
  let downX = 0;
  let downY = 0;
  let moved = false;

  function toSim(e: PointerEvent): { x: number; y: number } {
    const rect = svgEl.getBoundingClientRect();
    return {
      x: (e.clientX - rect.left - tx) / scale,
      y: (e.clientY - rect.top - ty) / scale,
    };
  }
  function nodeDown(e: PointerEvent, n: SimNode) {
    e.stopPropagation();
    dragging = n;
    moved = false;
    downX = e.clientX;
    downY = e.clientY;
    sim?.alphaTarget(0.3).restart();
    const p = toSim(e);
    n.fx = p.x;
    n.fy = p.y;
    svgEl.setPointerCapture(e.pointerId);
  }
  function dragMove(e: PointerEvent) {
    if (!dragging) return;
    if (Math.hypot(e.clientX - downX, e.clientY - downY) > 4) moved = true;
    const p = toSim(e);
    dragging.fx = p.x;
    dragging.fy = p.y;
  }
  function dragEnd() {
    if (!dragging) return;
    sim?.alphaTarget(0);
    const n = dragging;
    n.fx = null;
    n.fy = null;
    if (!moved) openNode(n);
    dragging = null;
  }
</script>

<section style="height:100vh;overflow:hidden;position:relative">
  {#if loading}
    <div class="nb-card" style="margin:32px">Loading graph…</div>
  {:else if error}
    <div class="nb-card" style="margin:32px;background:#c0392b;color:#fff">{error}</div>
  {:else if nodes.length === 0}
    <div class="nb-card" style="margin:32px">No concepts yet — capture something from Home.</div>
  {:else}
    <svg
      bind:this={svgEl}
      width="100%"
      height="100%"
      style="display:block;background:var(--paper);touch-action:none;cursor:grab"
      on:wheel={onWheel}
      on:pointerdown={canvasDown}
      on:pointermove={canvasMove}
      on:pointerup={canvasUp}
      role="application"
      aria-label="Concept graph"
    >
      <g transform="translate({tx},{ty}) scale({scale})">
        {#each links as l}
          {@const sn = asNode(l.source)}
          {@const tn = asNode(l.target)}
          {#if sn && tn}
            <line
              x1={sn.x ?? 0}
              y1={sn.y ?? 0}
              x2={tn.x ?? 0}
              y2={tn.y ?? 0}
              stroke="#111"
              stroke-width="3"
              opacity={dimmedLink(l) ? 0.12 : 1}
            />
          {/if}
        {/each}
        {#each nodes as n}
          <g
            transform="translate({n.x ?? 0},{n.y ?? 0})"
            style="cursor:pointer"
            opacity={dimmedNode(n.path) ? 0.25 : 1}
            role="button"
            tabindex="0"
            aria-label={n.title}
            on:pointerdown={(e) => nodeDown(e, n)}
            on:pointerenter={() => enterNode(n)}
            on:pointerleave={leaveNode}
            on:keydown={(e) => e.key === "Enter" && openNode(n)}
          >
            <circle cx="3" cy="3" r={n.r} fill="#111" />
            <circle r={n.r} fill={fillNode(n.path)} stroke="#111" stroke-width="3" />
            <text
              y={n.r + 16}
              text-anchor="middle"
              font-family="sans-serif"
              font-size="12"
              font-weight="800"
              fill="#111">{n.title}</text
            >
          </g>
        {/each}
      </g>
    </svg>
  {/if}
</section>
```

- [ ] **Step 4: Render the route**

In `src/routes/+page.svelte`, add the import after the `Settings` import (line 6):

```svelte
  import Graph from "$lib/components/Graph.svelte";
```

And add the render block after the `settings` block (line 17):

```svelte
    {#if $route==="graph"}<Graph />{/if}
```

- [ ] **Step 5: Type-check**

Run: `npm run check`
Expected: 0 errors. (Resolve any d3-force typing or Svelte a11y *errors*; a11y *warnings* are acceptable, matching the existing components. If `asNode`'s cast triggers a type error, ensure `SimLink["source"]` is used as the parameter type as written.)

- [ ] **Step 6: Run the full frontend suite + build**

Run: `npm run test && npm run build`
Expected: all vitest tests pass; SPA build succeeds into `build/`.

- [ ] **Step 7: Commit**

```bash
git add src/lib/components/Graph.svelte src/lib/stores.ts src/lib/components/Rail.svelte src/routes/+page.svelte
git commit -m "$(printf 'feat: add concept graph view and Graph route\n\nCo-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>')"
```

---

## Final Verification (after all tasks)

Run the full gate suite:

```bash
cd src-tauri && $HOME/.cargo/bin/cargo test && $HOME/.cargo/bin/cargo clippy --all-targets && $HOME/.cargo/bin/cargo fmt --check
cd .. && npm run check && npm run test && npm run build
```

Expected: Rust unit + integration tests green, clippy clean, fmt unchanged; svelte-check 0 errors; vitest green; SPA build succeeds.

**Manual smoke (optional, `npm run tauri dev`):** open the Graph route from the Rail; confirm nodes/edges render, hovering highlights neighbors, dragging a node re-settles the sim, wheel zooms, dragging the canvas pans, clicking a node opens it in Browse, and an empty wiki shows the empty state.
