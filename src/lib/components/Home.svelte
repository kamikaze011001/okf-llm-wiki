<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listPages, submitSource, type PageDto } from "$lib/api";
  import { route, currentPage, capturePrefill } from "$lib/stores";
  import Spinner from "$lib/components/Spinner.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  let input = ""; let note = ""; let busy = false; let pages: PageDto[] = [];
  let inputEl: HTMLInputElement;
  let loadingList = true;
  onMount(async () => { try { pages = await listPages(); } finally { loadingList = false; } });

  // Apply a pending prefill (from a drop/paste), then clear it.
  $: applyPrefill($capturePrefill);
  function applyPrefill(p: string) {
    if (p) { input = p; capturePrefill.set(""); }
  }
  // Focus the capture input when the capture route becomes active.
  $: if ($route === "capture" && inputEl) focusInput();
  async function focusInput() { await tick(); inputEl?.focus(); }
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
    <input class="nb-input" bind:this={inputEl} placeholder="Paste a link or write a note…" bind:value={input} />
    <input class="nb-input" style="margin-top:8px" placeholder="Why are you saving this? (optional)" bind:value={note} />
    <button class="nb-btn accent" style="margin-top:12px" on:click={go} disabled={busy}>Capture</button>
    {#if busy}<div style="margin-top:12px"><Spinner label="Digesting…" /></div>{/if}
  </div>
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
</section>
