<script lang="ts">
  import { askQuestion, type AnswerDto } from "$lib/api";
  let q = ""; let busy = false; let answer: AnswerDto | undefined;
  async function send(){ if(!q.trim()) return; busy = true; try { answer = await askQuestion(q); } finally { busy = false; } }
</script>
<section style="padding:32px;max-width:720px;margin:0 auto">
  <h1>Ask your wiki</h1>
  <div class="nb-card">
    <input class="nb-input" placeholder="Ask anything from your knowledge…" bind:value={q} on:keydown={(e)=> e.key==="Enter" && send()} />
    <button class="nb-btn accent" style="margin-top:12px" on:click={send} disabled={busy}>{busy?"Thinking…":"Ask"}</button>
  </div>
  {#if answer}
    <article class="nb-card" style="margin-top:16px;white-space:pre-wrap">{answer.text}</article>
    <h3 style="margin-top:12px">Sources</h3>
    {#each answer.citations as c}<span class="nb-chip">{c}</span>{/each}
  {/if}
</section>
