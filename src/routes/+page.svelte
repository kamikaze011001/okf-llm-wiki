<script lang="ts">
  import Rail from "$lib/components/Rail.svelte";
  import Home from "$lib/components/Home.svelte";
  import Browse from "$lib/components/Browse.svelte";
  import Ask from "$lib/components/Ask.svelte";
  import Settings from "$lib/components/Settings.svelte";
  import Graph from "$lib/components/Graph.svelte";
  import { route } from "$lib/stores";
</script>

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
