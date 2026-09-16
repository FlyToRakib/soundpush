<script lang="ts">
  import { t } from "../i18n";

  let { db, label, clipping = false }: { db: number; label: string; clipping?: boolean } = $props();
  // Map −60…0 dBFS to 0…100 %.
  const pct = $derived(Math.max(0, Math.min(100, ((db + 60) / 60) * 100)));
  const rounded = $derived(Math.max(-60, Math.min(0, Math.round(db))));
  // The limiter is working, so the level is being held back: say so, don't just show red.
  const text = $derived(clipping ? `${t("a11y.level", rounded)}, ${t("audio.clipping")}` : t("a11y.level", rounded));
</script>

<div class="wrap">
  <div
    class="meter"
    role="meter"
    aria-label={label}
    aria-valuemin={-60}
    aria-valuemax={0}
    aria-valuenow={rounded}
    aria-valuetext={text}
  >
    <div class="fill" class:hot={db > -3} style="width: {pct}%"></div>
  </div>
  {#if clipping}
    <span class="clip" title={t("audio.clipping.desc")} aria-hidden="true">{t("audio.clipping")}</span>
  {/if}
</div>

<style>
  .wrap {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
  }
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
  .clip {
    color: var(--color-danger);
    font-size: var(--font-caption-size);
    line-height: var(--font-caption-line);
    font-weight: 600;
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
