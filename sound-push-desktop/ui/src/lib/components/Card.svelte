<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, description, children }: { title?: string; description?: string; children: Snippet } = $props();
</script>

<section class="card">
  {#if title}
    <header>
      <h2>{title}</h2>
      {#if description}<p class="caption">{description}</p>{/if}
    </header>
  {/if}
  {@render children()}
</section>

<style>
  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    padding: var(--space-md);
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }
  header {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
</style>
