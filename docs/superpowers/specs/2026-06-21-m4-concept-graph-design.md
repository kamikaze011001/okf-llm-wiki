# M4 Concept-Graph — Design

**Status:** Approved 2026-06-21
**Milestone:** M4 (Zero-barrier UX) — deferred "browsable concept graph" piece from M3
**Branch:** `feat/m4-concept-graph`

## 1. Summary

A new standalone **Graph** route that renders the whole wiki as one force-directed
node-link diagram. Each node is an existing OKF concept page; each edge is a `[[link]]`
between two pages. Clicking a node opens it in Browse. The graph fills the pane
(full-bleed), supports pan/zoom, node-drag, and hover-highlight, and is styled in the
app's neo-brutalist vocabulary.

This is the "Karpathy LLM-Wiki soul" visualization deferred out of M3.

## 2. Decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Layout/feel | **A — Full-bleed map**: whole wiki as one force graph, standalone route, click node → open in Browse |
| Rendering | **d3-force for layout math + custom neo-brutalist SVG** for drawing |
| Ghost nodes | **No** — existing pages only; unresolved `[[links]]` do not appear |
| Interactions | **Pan & zoom, hover-highlight, drag nodes** (plus click-to-open) |

## 3. Backend — read-only, no new IO

The link graph is **already kept fresh** in `AppState.links` (a `Mutex<LinkGraph>`,
rebuilt by `refresh_index_and_links` after every capture/edit/delete/create). The graph
view only reads it — no store IO, no LLM, no `.await`.

### 3.1 `core/links.rs` (Tauri-free)

Add plain structs and a method on `LinkGraph`:

```rust
pub struct GraphNode {
    pub path: String,    // e.g. "concepts/sleep.md"
    pub title: String,
    pub degree: usize,   // number of edges touching this node (undirected)
}

pub struct GraphEdge {
    pub source: String,  // page path
    pub target: String,  // page path
}

pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl LinkGraph {
    pub fn graph_data(&self) -> GraphData { /* ... */ }
}
```

`graph_data` rules:

- **Nodes:** one per existing concept page (every entry in `slug_to_path`), titled via
  `slug_to_title`. Orphan pages (degree 0, no links in or out) are included.
- **Edges:** derived from the existing `backlinks` map (`target_slug -> Vec<source_path>`).
  For each `(target_slug, source_paths)`, resolve `target_slug` to a path via
  `path_for`; for each `source_path`, emit an edge `source_path -> target_path`.
  - **Undirected & deduped:** a mutual A↔B link yields exactly one edge. Normalize each
    pair (e.g. order the two paths) and collect into a set before emitting.
  - **Skip self-links** (`source == target`).
  - **Skip edges to unresolved targets** (`path_for` returns `None`).
- **Degree:** computed from the final deduped edge set — count of edges touching each path.

This is implementable entirely from `LinkGraph`'s existing fields; no change to
`build_link_graph` is required.

### 3.2 `commands.rs` — new `get_graph` command

```rust
#[derive(Serialize)]
pub struct GraphNodeDto { pub path: String, pub title: String, pub degree: usize }

#[derive(Serialize)]
pub struct GraphEdgeDto { pub source: String, pub target: String }

#[derive(Serialize)]
pub struct GraphDto { pub nodes: Vec<GraphNodeDto>, pub edges: Vec<GraphEdgeDto> }

#[tauri::command]
pub fn get_graph(state: State<AppState>) -> Result<GraphDto, String> {
    let graph = state.links.lock().unwrap();
    let data = graph.graph_data();
    // map GraphData -> GraphDto
    Ok(/* ... */)
}
```

- Synchronous (mirrors `get_page_view`): locks `state.links`, maps, returns. No
  `MutexGuard` held across an `.await` because there is no `.await`.
- Register in the Tauri builder's `invoke_handler` alongside the other commands.

### 3.3 Security review

Not required for this slice. The change touches `core/links.rs` and `commands.rs` only —
**none** of the sensitive areas (`settings.rs`, `state.rs`, `store.rs`, `provider/`). The
new command reads in-memory state and performs no file IO or outbound requests.

## 4. Frontend

| File | Change |
|---|---|
| `src/lib/stores.ts` | Add `"graph"` to the `Route` union. |
| `src/routes/+page.svelte` | Render `<Graph/>` when `$route === "graph"`. |
| `src/lib/components/Rail.svelte` | Add `{ id: "graph", label: "Graph" }` to the nav items. |
| `src/lib/api.ts` | Add `getGraph()` wrapper + `GraphNode` / `GraphEdge` / `GraphData` types. |
| `src/lib/components/Graph.svelte` | New component — the graph view. |
| `package.json` | Add `d3-force` (dep) + `@types/d3-force` (devDep). |

### 4.1 `api.ts`

```ts
export interface GraphNode { path: string; title: string; degree: number; }
export interface GraphEdge { source: string; target: string; }
export interface GraphData { nodes: GraphNode[]; edges: GraphEdge[]; }
export const getGraph = () => invoke<GraphData>("get_graph");
```

### 4.2 `Graph.svelte`

- On mount: `getGraph()`. While pending, show a neo-brutalist loading state.
- **Model shaping** is a pure, exported helper `buildGraphModel(data: GraphData)` that
  turns `GraphData` into the simulation's node/link arrays (assigning initial positions,
  link source/target references by `path`, node radius by degree). Kept separate from
  rendering so it is unit-testable.
- **Simulation:** `d3-force` — `forceManyBody` (repulsion), `forceLink` keyed on `path`,
  `forceCenter`, `forceCollide` (radius from node size) to prevent overlap. Each tick
  writes node `x`/`y` into reactive Svelte state; the SVG re-renders from it.
- **Dependency scope:** only `d3-force`. Pan/zoom and node-drag are hand-rolled with an
  SVG `<g transform="translate(...) scale(...)">` plus pointer/wheel handlers — no
  `d3-zoom`, `d3-drag`, or `d3-selection`.

### 4.3 Visual (neo-brutalist)

- **Nodes:** white box, `3px solid var(--ink)` border, hard offset shadow, label inside —
  same vocabulary as existing chips/cards. Higher-degree (hub) nodes render slightly
  larger. Hovered node + its direct neighbors get an accent (`--blue`) fill; all other
  nodes/edges dim. `cursor: pointer`.
- **Edges:** plain `3px` solid `var(--ink)` lines, no arrowheads.
- **Interactions:**
  - Click node → `currentPage.set(node.path)` then `route.set("browse")`.
  - Hover node → highlight its neighbors, dim the rest.
  - Drag node → pin via d3 `fx`/`fy`; sim re-settles; release unpins.
  - Drag empty canvas → pan (update the group transform).
  - Wheel → zoom (update the group transform scale, clamped).
- **States:** loading (spinner/placeholder); empty (`"No concepts yet — capture
  something from Home"` in a neo-brutalist card); single page sits centered.

## 5. Testing

### 5.1 Rust (`core/links.rs` unit tests)

Using the process-wide `AtomicU64` temp-dir isolation pattern (`okf-{pid}-{n}`), build a
store with known pages and assert `graph_data`:

- node count == number of existing pages; orphan page is present with `degree == 0`.
- edge derived from a `[[link]]` connects the right two paths.
- a mutual A↔B link produces exactly **one** edge (dedup).
- a self-link (`[[Self]]` on its own page) produces **no** edge.
- a `[[link]]` to a non-existent page produces **no** edge and **no** node.
- degree counts match the deduped edge set.

### 5.2 Frontend (vitest)

- `getGraph` wrapper calls `invoke("get_graph")` and returns its result (mocked `invoke`).
- `buildGraphModel(data)` shapes nodes/links correctly: link endpoints reference nodes by
  `path`, node radius increases with degree, empty input yields empty model.

The d3 simulation and SVG rendering are not unit-tested (integration-level concern); the
testable logic is isolated in `buildGraphModel`.

## 6. Out of scope (v1) — clean follow-ups

- Ghost / red-link nodes in the graph (create-on-click from the graph).
- In-graph search / filter / focus-on-node.
- Persisted node positions across sessions.
- Directed edges / arrowheads.
- Clustering / community detection / coloring by tag.

## 7. Acceptance

- A **Graph** entry appears in the Rail; selecting it shows the force graph of all
  existing concept pages with their `[[link]]` edges.
- Clicking a node opens that page in Browse.
- Pan, zoom, node-drag, and hover-highlight all work.
- Empty wiki shows the empty state; a wiki with pages shows a settled, readable graph.
- `get_graph` adds no file IO and holds no lock across an `.await`.
- All gates green: `cargo test`, `cargo clippy --all-targets`, `cargo fmt`, `npm run check`,
  `npm run test`, `npm run build`.
