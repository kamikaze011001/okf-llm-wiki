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
  function canvasCancel() {
    panning = false;
    if (!dragging) return;
    leaveNode();
    sim?.alphaTarget(0);
    dragging.fx = null;
    dragging.fy = null;
    dragging = null;
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
    leaveNode();
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
      on:pointercancel={canvasCancel}
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
            on:keydown={(e) => (e.key === "Enter" || e.key === " ") && openNode(n)}
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
