<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, getPageView, updatePage, deletePage, type PageDto, type PageView } from "$lib/api";
  import { currentPage } from "$lib/stores";
  let pages: PageDto[] = [];
  let view: PageView | undefined;
  let mounted = false;
  let mode: "view" | "edit" = "view";
  let saving = false;
  let editError = "";
  let confirmingDelete = false;
  let deleting = false;
  let deleteError = "";
  // Edit form fields (seeded from `view` when entering edit mode).
  let editTitle = "";
  let editTags = "";
  let editNote = "";
  let editBody = "";
  onMount(async () => { pages = await listPages(); mounted = true; });
  $: selectedPath = $currentPage ?? pages[0]?.path ?? null;
  $: if (mounted) loadFor(selectedPath);
  async function loadFor(path: string | null) {
    if (!path) { view = undefined; return; }
    view = await getPageView(path);
    mode = "view";
    confirmingDelete = false;
  }
  function go(path: string) { currentPage.set(path); }
  function startEdit() {
    if (!view) return;
    editTitle = view.title;
    editTags = view.tags.join(", ");
    editNote = view.note ?? "";
    editBody = view.body;
    editError = "";
    mode = "edit";
  }
  async function saveEdit() {
    if (!view) return;
    saving = true;
    editError = "";
    try {
      const tags = editTags.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
      await updatePage(view.path, editTitle || undefined, tags, editNote || undefined, editBody);
      view = await getPageView(view.path);
      mode = "view";
    } catch (e) {
      editError = String(e);
    } finally {
      saving = false;
    }
  }
  async function confirmDelete() {
    if (!view) return;
    deleting = true;
    deleteError = "";
    try {
      await deletePage(view.path);
      pages = await listPages();
      currentPage.set(null);
    } catch (e) {
      deleteError = String(e);
    } finally {
      deleting = false;
      confirmingDelete = false;
    }
  }
  function cancelDelete() { confirmingDelete = false; }
</script>
<section style="padding:32px;max-width:760px;margin:0 auto">
  {#if view && mode === "view"}
    <div style="display:flex;gap:8px;justify-content:flex-end;margin-bottom:8px">
      <button class="nb-btn" on:click={startEdit}>Edit</button>
      {#if confirmingDelete}
        <span style="align-self:center">Confirm delete?</span>
        <button class="nb-btn" style="background:#c0392b;color:#fff" on:click={confirmDelete} disabled={deleting}>{deleting ? "Deleting…" : "Yes"}</button>
        <button class="nb-btn" on:click={cancelDelete} disabled={deleting}>No</button>
      {:else}
        <button class="nb-btn" on:click={() => (confirmingDelete = true)}>Delete</button>
      {/if}
    </div>
    {#if deleteError}<div class="nb-card" style="background:#c0392b;color:#fff;margin:0 0 8px 0">{deleteError}</div>{/if}
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
  {:else if view && mode === "edit"}
    <h2>Edit page</h2>
    {#if editError}<div class="nb-card" style="background:#c0392b;color:#fff;margin:8px 0">{editError}</div>{/if}
    <label style="display:block;margin-top:8px">Title<br /><input class="nb-input" style="width:100%" bind:value={editTitle} /></label>
    <label style="display:block;margin-top:8px">Tags (comma-separated)<br /><input class="nb-input" style="width:100%" bind:value={editTags} /></label>
    <label style="display:block;margin-top:8px">Note<br /><input class="nb-input" style="width:100%" bind:value={editNote} /></label>
    <label style="display:block;margin-top:8px">Body (Markdown)<br /><textarea class="nb-input" style="width:100%;min-height:240px;font-family:monospace" bind:value={editBody}></textarea></label>
    <div style="display:flex;gap:8px;margin-top:12px">
      <button class="nb-btn" on:click={saveEdit} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
      <button class="nb-btn" on:click={() => (mode = "view")} disabled={saving}>Cancel</button>
    </div>
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
