<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, type PageDto } from "$lib/api";
  import { currentPage } from "$lib/stores";
  let pages: PageDto[] = []; let selected: PageDto | undefined;
  onMount(async () => { pages = await listPages(); pick(); });
  $: pick();
  function pick(){ selected = pages.find(p => p.path === $currentPage) ?? pages[0]; }
</script>
<section style="padding:32px;max-width:760px;margin:0 auto">
  {#if selected}
    <span class="nb-chip" style="background:var(--pink);color:#fff">CONCEPT</span>
    <h1>{selected.title}</h1>
    <div>{#each selected.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
    {#if selected.note}<div class="nb-card" style="background:var(--yellow);margin:12px 0"><strong>★ Your note:</strong> {selected.note}</div>{/if}
    <article class="nb-card" style="margin-top:12px;white-space:pre-wrap">{selected.body}</article>
    {#if selected.resource}<p style="margin-top:12px"><a href={selected.resource} target="_blank">Open source ↗</a></p>{/if}
  {:else}
    <p>No pages yet — capture something from Home.</p>
  {/if}
</section>
