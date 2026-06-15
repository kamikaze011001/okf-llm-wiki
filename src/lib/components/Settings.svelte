<script lang="ts">
  import { onMount } from "svelte";
  import { getSettings, setSettings, type Settings } from "$lib/api";
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"" };
  let saved = false;
  onMount(async () => { s = await getSettings(); });
  async function save(){ await setSettings(s); saved = true; setTimeout(()=>saved=false, 1500); }
</script>
<section style="padding:32px;max-width:560px;margin:0 auto">
  <h1>Settings</h1>
  <div class="nb-card" style="display:grid;gap:10px">
    <label>Provider<select class="nb-input" bind:value={s.provider}><option>claude</option><option>openai</option><option>ollama</option></select></label>
    <label>Model<input class="nb-input" bind:value={s.model} /></label>
    <label>API key<input class="nb-input" type="password" bind:value={s.api_key} /></label>
    <label>Wiki folder<input class="nb-input" bind:value={s.wiki_path} placeholder="/Users/you/wiki" /></label>
    <button class="nb-btn accent" on:click={save}>{saved?"Saved ✓":"Save"}</button>
  </div>
</section>
