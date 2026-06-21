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
  import { route, configured } from "$lib/stores";
  import { getSettings } from "$lib/api";
  import { isConfigured } from "$lib/capture";

  let loading = true;
  onMount(async () => {
    try {
      const s = await getSettings();
      configured.set(isConfigured(s));
    } finally {
      loading = false;
    }
  });
</script>

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
