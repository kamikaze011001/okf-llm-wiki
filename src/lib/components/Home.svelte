<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, submitSource, type PageDto } from "$lib/api";
  import { route, currentPage } from "$lib/stores";
  let input = ""; let note = ""; let busy = false; let pages: PageDto[] = [];
  onMount(async () => { pages = await listPages(); });
  async function go() {
    if (!input.trim()) return;
    busy = true;
    try { await submitSource(input, note || undefined); input=""; note=""; pages = await listPages(); }
    finally { busy = false; }
  }
</script>
<section style="padding:32px;max-width:720px;margin:0 auto">
  <h1>Good morning</h1>
  <div class="nb-card" style="margin:16px 0">
    <input class="nb-input" placeholder="Paste a link or write a note…" bind:value={input} />
    <input class="nb-input" style="margin-top:8px" placeholder="Why are you saving this? (optional)" bind:value={note} />
    <button class="nb-btn accent" style="margin-top:12px" on:click={go} disabled={busy}>{busy ? "Digesting…" : "Capture"}</button>
  </div>
  <h3>Recent</h3>
  {#each pages as p}
    <button class="nb-card" style="display:block;width:100%;text-align:left;margin-bottom:8px;cursor:pointer"
      on:click={() => { currentPage.set(p.path); route.set("browse"); }}>
      <strong>{p.title}</strong>
      <div>{#each p.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
    </button>
  {/each}
</section>
