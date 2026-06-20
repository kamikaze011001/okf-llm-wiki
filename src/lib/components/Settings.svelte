<script lang="ts">
  import { onMount } from "svelte";
  import { getSettings, setSettings, reindex, type Settings } from "$lib/api";
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"", embed_provider:"hash", embed_model:"nomic-embed-text", ollama_url:"http://localhost:11434" };
  let saved = false;
  let error = "";
  let reindexing = false;
  let reindexError = "";
  onMount(async () => { s = await getSettings(); });
  async function save(){
    error = "";
    try {
      await setSettings(s);
      saved = true;
      setTimeout(()=>saved=false, 1500);
    } catch (e) {
      error = String(e);
    }
  }
  async function runReindex(){
    reindexError = "";
    reindexing = true;
    try {
      await reindex();
    } catch (e) {
      reindexError = String(e);
    } finally {
      reindexing = false;
    }
  }
</script>
<section style="padding:32px;max-width:560px;margin:0 auto">
  <h1>Settings</h1>
  <div class="nb-card" style="display:grid;gap:10px">
    <label>Provider<select class="nb-input" bind:value={s.provider}><option>claude</option><option>openai</option><option>ollama</option></select></label>
    <label>Model<input class="nb-input" bind:value={s.model} /></label>
    <label>API key<input class="nb-input" type="password" bind:value={s.api_key} /></label>
    <label>Wiki folder<input class="nb-input" bind:value={s.wiki_path} placeholder="/Users/you/wiki" /></label>
    <label>Embedding<select class="nb-input" bind:value={s.embed_provider}><option value="hash">hash (offline)</option><option value="ollama">ollama</option></select></label>
    {#if s.embed_provider === "ollama"}
      <label>Ollama URL<input class="nb-input" bind:value={s.ollama_url} placeholder="http://localhost:11434" /></label>
      <label>Embedding model<input class="nb-input" bind:value={s.embed_model} placeholder="nomic-embed-text" /></label>
    {/if}
    <button class="nb-btn accent" on:click={save}>{saved?"Saved ✓":"Save"}</button>
    {#if error}<p style="color:var(--pink);font-weight:700">⚠ {error}</p>{/if}
    <button class="nb-btn" on:click={runReindex} disabled={reindexing}>{reindexing?"Reindexing…":"Reindex wiki"}</button>
    {#if reindexError}<p style="color:var(--pink);font-weight:700">⚠ {reindexError}</p>{/if}
  </div>
</section>
