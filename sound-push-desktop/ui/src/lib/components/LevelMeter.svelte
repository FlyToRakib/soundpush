<script lang="ts">
  import { t } from "../i18n";

  let { db, label }: { db: number; label: string } = $props();
  // Map −60…0 dBFS to 0…100 %.
  const pct = $derived(Math.max(0, Math.min(100, ((db + 60) / 60) * 100)));
  const rounded = $derived(Math.max(-60, Math.min(0, Math.round(db))));
</script>

<div
  class="meter"
  role="meter"
  aria-label={label}
  aria-valuemin={-60}
  aria-valuemax={0}
  aria-valuenow={rounded}
  aria-valuetext={t("a11y.level", rounded)}
>
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
  @media (forced-colors: active) {
    .meter {
      border: 1px solid CanvasText;
    }
    .fill {
      background: Highlight;
    }
  }
</style>
