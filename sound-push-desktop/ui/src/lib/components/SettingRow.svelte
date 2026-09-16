<script lang="ts">
  import type { Snippet } from "svelte";

  let { label, description, id, children }: { label: string; description?: string; id?: string; children: Snippet } = $props();
</script>

<div class="setting">
  <div class="text">
    <label for={id}>{label}</label>
    {#if description}<p class="caption">{description}</p>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .setting {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    min-height: 40px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  label {
    font-weight: 500;
  }
  .control {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
</style>
