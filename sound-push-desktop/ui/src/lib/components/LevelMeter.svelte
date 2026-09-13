<script lang="ts">
  let { db, label }: { db: number; label: string } = $props();
  // Map −60…0 dBFS to 0…100 %.
  const pct = $derived(Math.max(0, Math.min(100, ((db + 60) / 60) * 100)));
</script>

<div class="meter" role="meter" aria-label={label} aria-valuemin={-60} aria-valuemax={0} aria-valuenow={Math.round(db)}>
  <div class="fill" class:hot={db > -3} style="width: {pct}%"></div>
</div>

<style>
  .meter {
    width: 180px;
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-muted);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--color-success);
    transition: width 80ms linear;
  }
  .fill.hot {
    background: var(--color-danger);
  }
</style>
