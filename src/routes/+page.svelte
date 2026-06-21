<script lang="ts">
  import { onMount } from "svelte";
  import Rail from "$lib/components/Rail.svelte";
  import Home from "$lib/components/Home.svelte";
  import Browse from "$lib/components/Browse.svelte";
  import Ask from "$lib/components/Ask.svelte";
  import Settings from "$lib/components/Settings.svelte";
  import Graph from "$lib/components/Graph.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import Spinner from "$lib/components/Spinner.svelte";
  import { route, configured, capturePrefill } from "$lib/stores";
  import { getSettings } from "$lib/api";
  import { isConfigured, extractDropText, extractPasteText, shouldInterceptPaste } from "$lib/capture";

  let loading = true;
  onMount(async () => {
    try {
      const s = await getSettings();
      configured.set(isConfigured(s));
    } finally {
      loading = false;
    }
  });

  function onKeydown(e: KeyboardEvent) {
    if (!$configured) return;
    if ((e.metaKey || e.ctrlKey) && (e.key === "n" || e.key === "N")) {
      e.preventDefault();
      route.set("capture");
    }
  }
  function onDragOver(e: DragEvent) {
    if (!$configured) return;
    e.preventDefault(); // allow the drop event to fire
  }
  function onDrop(e: DragEvent) {
    if (!$configured) return;
    const text = extractDropText(e.dataTransfer);
    if (!text) return;
    e.preventDefault();
    capturePrefill.set(text);
    route.set("capture");
  }
  function onPaste(e: ClipboardEvent) {
    if (!$configured) return;
    if (!shouldInterceptPaste(e.target)) return;
    const text = extractPasteText(e);
    if (!text) return;
    e.preventDefault();
    capturePrefill.set(text);
    route.set("capture");
  }
</script>

<svelte:window on:keydown={onKeydown} on:dragover={onDragOver} on:drop={onDrop} on:paste={onPaste} />

{#if loading}
  <main style="display:flex;align-items:center;justify-content:center;height:100vh">
    <Spinner label="Loading…" />
  </main>
{:else if !$configured}
  <Onboarding />
{:else}
  <main style="display:flex">
    <Rail />
    <div style="flex:1">
      {#if $route==="home"}<Home />{/if}
      {#if $route==="capture"}<Home />{/if}
      {#if $route==="browse"}<Browse />{/if}
      {#if $route==="ask"}<Ask />{/if}
      {#if $route==="settings"}<Settings />{/if}
      {#if $route==="graph"}<Graph />{/if}
    </div>
  </main>
{/if}
