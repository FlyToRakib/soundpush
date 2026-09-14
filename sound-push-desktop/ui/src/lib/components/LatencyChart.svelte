<script lang="ts">
  // Latency (line) and packet loss (bars) over the last minutes, with a text equivalent
  // (plan §23.4 LatencyChart, §23.5 "charts expose textual equivalents").
  import { HISTORY_MS, summarize, type Sample } from "../connection";
  import { formatNumber, t } from "../i18n";

  let { samples }: { samples: readonly Sample[] } = $props();

  const W = 300;
  const H = 64;
  const summary = $derived(summarize(samples));
  const scaleMs = $derived(Math.max(50, summary?.maxMs ?? 0) * 1.15);
  const scaleLoss = $derived(Math.max(5, summary?.maxLossPct ?? 0));
  const end = $derived(samples[samples.length - 1]?.at ?? 0);
  const x = (at: number) => W - ((end - at) / HISTORY_MS) * W;
  const line = $derived(
    samples.map((s) => `${x(s.at).toFixed(1)},${(H - (s.latencyMs / scaleMs) * H).toFixed(1)}`).join(" "),
  );
  const bars = $derived(samples.filter((s) => s.lossPct > 0).map((s) => ({ x: x(s.at), h: (s.lossPct / scaleLoss) * H * 0.5 })));
  const label = $derived(
    summary
      ? t(
          "chart.summary",
          Math.round(summary.minMs),
          Math.round(summary.avgMs),
          Math.round(summary.maxMs),
          formatNumber(summary.maxLossPct, { maximumFractionDigits: 1 }),
        )
      : t("chart.empty"),
  );
</script>

<figure class="chart">
  <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label={label}>
    <line class="grid" x1="0" x2={W} y1={H - (50 / scaleMs) * H} y2={H - (50 / scaleMs) * H} />
    {#each bars as bar, i (i)}
      <rect class="loss" x={bar.x - 1} y={H - bar.h} width="2" height={bar.h} />
    {/each}
    {#if samples.length > 1}<polyline class="latency" points={line} />{/if}
  </svg>
  <figcaption class="caption" aria-hidden="true">{label}</figcaption>
</figure>

<style>
  .chart {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  svg {
    width: 100%;
    height: 64px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-background);
  }
  .latency {
    fill: none;
    stroke: var(--color-accent);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .loss {
    fill: var(--color-warning);
  }
  .grid {
    stroke: var(--color-border);
    stroke-dasharray: 4 4;
    vector-effect: non-scaling-stroke;
  }
  @media (forced-colors: active) {
    .latency {
      stroke: CanvasText;
    }
    .loss {
      fill: Highlight;
    }
  }
</style>
