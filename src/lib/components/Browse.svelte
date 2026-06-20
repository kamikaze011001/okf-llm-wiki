<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, getPageView, type PageDto, type PageView } from "$lib/api";
  import { currentPage } from "$lib/stores";
  let pages: PageDto[] = [];
  let view: PageView | undefined;
  let mounted = false;
  onMount(async () => { pages = await listPages(); mounted = true; });
  // Derive the path to show from the selected page (or the first page as fallback),
  // then load it once the page list is available. Referencing the store + `pages`
  // here is what makes Svelte re-run these statements when either changes.
  $: selectedPath = $currentPage ?? pages[0]?.path ?? null;
  $: if (mounted) loadFor(selectedPath);
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    view = await getPageView(path);
  }
  function go(path: string) { currentPage.set(path); }
</script>
<section style="padding:32px;max-width:760px;margin:0 auto">
  {#if view}
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
  {:else}
    <p>No pages yet — capture something from Home.</p>
  {/if}
</section>

<style>
  .nb-wikilink {
    color: var(--pink);
    font-weight: 700;
    text-decoration: underline;
    text-decoration-thickness: 2px;
    cursor: pointer;
  }
  .nb-redlink {
    color: #c0392b;
    font-weight: 700;
    text-decoration: underline dotted;
    cursor: not-allowed;
  }
</style>
