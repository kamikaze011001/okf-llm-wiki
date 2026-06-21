<script lang="ts">
  export let label = "";
</script>

<div class="nb-spin-wrap" role="status" aria-label={label || "Loading"}>
  <span class="nb-spinner" aria-hidden="true"></span>
  {#if label}<span class="nb-spin-label">{label}</span>{/if}
</div>

<style>
  .nb-spin-wrap { display: inline-flex; align-items: center; gap: 10px; }
  .nb-spin-label { font-weight: 700; }
  .nb-spinner {
    width: 18px; height: 18px;
    border: 3px solid var(--ink);
    border-top-color: var(--blue);
    display: inline-block;
    animation: nb-spin 0.8s steps(8) infinite;
  }
  @keyframes nb-spin { to { transform: rotate(360deg); } }
</style>
