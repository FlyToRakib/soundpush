<script lang="ts">
  import type { LinkQuality } from "../engine/types";
  import { t } from "../i18n";

  let { quality, latencyMs }: { quality: LinkQuality; latencyMs?: number } = $props();
</script>

{#if quality !== "unknown"}
  <span class="badge {quality}">
    <span class="bars" aria-hidden="true">
      <i></i><i class:off={quality === "poor"}></i><i class:off={quality !== "excellent"}></i>
    </span>
    {t(`quality.${quality}`)}{#if latencyMs && latencyMs > 0}&nbsp;· {Math.round(latencyMs)} ms{/if}
  </span>
{/if}

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--font-caption-size);
    color: var(--color-text-secondary);
    font-variant-numeric: tabular-nums;
  }
  .bars {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 12px;
  }
  i {
    width: 3px;
    border-radius: 1px;
    background: currentColor;
  }
  i:nth-child(1) {
    height: 5px;
  }
  i:nth-child(2) {
    height: 8px;
  }
  i:nth-child(3) {
    height: 12px;
  }
  i.off {
    opacity: 0.25;
  }
  .excellent .bars,
  .good .bars {
    color: var(--color-success);
  }
  .poor .bars {
    color: var(--color-warning);
  }
</style>
