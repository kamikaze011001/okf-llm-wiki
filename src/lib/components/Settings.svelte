<script lang="ts">
  import { onMount } from "svelte";
  import { getSettings, setSettings, reindex, listOpenRouterModels, type Settings, type ModelInfo } from "$lib/api";
  import { filterModels } from "$lib/modelFilter";
  import Spinner from "$lib/components/Spinner.svelte";
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"", embed_provider:"hash", embed_model:"nomic-embed-text", ollama_url:"http://localhost:11434" };
  let saved = false;
  let error = "";
  let reindexing = false;
  let reindexError = "";

  // OpenRouter model picker state.
  let models: ModelInfo[] = [];
  let modelsLoaded = false;
  let loadingModels = false;
  let modelsError = "";
  let modelQuery = "";

  onMount(async () => { s = await getSettings(); });

  async function loadModels(){
    loadingModels = true;
    modelsError = "";
    try {
      models = await listOpenRouterModels();
      modelsLoaded = true;
    } catch (e) {
      modelsError = String(e);
    } finally {
      loadingModels = false;
    }
  }

  // Fetch the catalog the first time OpenRouter is selected. The `!modelsError`
  // guard stops an auto-retry loop after a failure (the Refresh button re-fetches).
  $: if (s.provider === "openrouter" && !modelsLoaded && !loadingModels && !modelsError) loadModels();
  $: filtered = filterModels(models, modelQuery);

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
    <label>Provider<select class="nb-input" bind:value={s.provider}><option>claude</option><option>openrouter</option></select></label>
    <label>Model<input class="nb-input" bind:value={s.model} placeholder={s.provider === "openrouter" ? "e.g. openai/gpt-4o" : "claude-opus-4-8"} /></label>
    {#if s.provider === "openrouter"}
      <div class="nb-card" style="background:var(--paper)">
        <div style="display:flex;justify-content:space-between;align-items:center;gap:8px">
          <strong>Browse models</strong>
          <button class="nb-btn" on:click={loadModels} disabled={loadingModels}>{loadingModels?"Loading…":"Refresh"}</button>
        </div>
        {#if loadingModels}
          <div style="margin-top:8px"><Spinner label="Fetching models…" /></div>
        {:else if modelsError}
          <p style="color:var(--pink);font-weight:700">⚠ {modelsError}</p>
        {:else if modelsLoaded}
          <input class="nb-input" style="width:100%;margin-top:8px" placeholder="Filter models…" bind:value={modelQuery} />
          {#if filtered.length}
            <ul style="list-style:none;margin:8px 0 0 0;padding:0;max-height:220px;overflow:auto">
              {#each filtered.slice(0, 100) as m (m.id)}
                <li>
                  <button class="nb-btn" style="width:100%;text-align:left;margin-top:4px;{s.model===m.id?'background:var(--blue);color:#fff':''}" on:click={() => (s.model = m.id)}>
                    <strong>{m.id}</strong>{#if m.name && m.name !== m.id} — {m.name}{/if}
                  </button>
                </li>
              {/each}
            </ul>
            {#if filtered.length > 100}<p style="margin-top:4px">Showing first 100 of {filtered.length}. Narrow your filter.</p>{/if}
          {:else}
            <p style="margin-top:8px">No models match "{modelQuery}".</p>
          {/if}
        {/if}
      </div>
    {/if}
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
