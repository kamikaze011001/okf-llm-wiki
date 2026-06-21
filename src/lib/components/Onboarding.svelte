<script lang="ts">
  import { setSettings, type Settings } from "$lib/api";
  import { configured } from "$lib/stores";
  import { isConfigured } from "$lib/capture";

  let s: Settings = {
    provider: "claude", model: "claude-opus-4-8", api_key: "", wiki_path: "",
    embed_provider: "hash", embed_model: "nomic-embed-text", ollama_url: "http://localhost:11434",
  };
  let busy = false;
  let error = "";

  async function start() {
    if (!isConfigured(s)) return;
    busy = true;
    error = "";
    try {
      await setSettings(s);
      configured.set(true);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<section style="min-height:100vh;display:flex;align-items:center;justify-content:center;padding:32px">
  <div class="nb-card" style="max-width:460px;width:100%;display:grid;gap:12px">
    <h1 style="margin:0">OKF Wiki</h1>
    <p style="margin:0">Paste a link or a note and it becomes a knowledge page you can browse and ask questions over. First, point it at a folder and add your Claude API key.</p>
    <label>API key<input class="nb-input" type="password" bind:value={s.api_key} placeholder="sk-ant-…" /></label>
    <label>Wiki folder<input class="nb-input" bind:value={s.wiki_path} placeholder="/Users/you/wiki" /></label>
    <button class="nb-btn accent" on:click={start} disabled={busy || !isConfigured(s)}>{busy ? "Setting up…" : "Get started"}</button>
    {#if error}<p style="color:var(--pink);font-weight:700;margin:0">⚠ {error}</p>{/if}
  </div>
</section>
