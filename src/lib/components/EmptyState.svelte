<script lang="ts">
  export let title: string;
  export let subtext = "";
</script>

<div class="nb-card" style="text-align:center;max-width:420px;margin:32px auto">
  <h3 style="margin:0 0 8px 0">{title}</h3>
  {#if subtext}<p style="margin:0 0 12px 0">{subtext}</p>{/if}
  <slot />
</div>
