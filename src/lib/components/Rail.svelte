<script lang="ts">
  import { route, type Route } from "$lib/stores";
  const items: {id: Route; label: string}[] = [
    {id:"home",label:"Home"},{id:"capture",label:"＋ Capture"},
    {id:"browse",label:"Browse"},{id:"ask",label:"Ask"},{id:"settings",label:"⚙ Settings"},
  ];
</script>
<nav style="width:150px;border-right:var(--border);background:var(--yellow);padding:12px;min-height:100vh">
  {#each items as it}
    <button class="nb-btn" style="display:block;width:100%;margin-bottom:8px;{$route===it.id?'background:var(--blue);color:#fff':''}"
      on:click={() => route.set(it.id)}>{it.label}</button>
  {/each}
</nav>
