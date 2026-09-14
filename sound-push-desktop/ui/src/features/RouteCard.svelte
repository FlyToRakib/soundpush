<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import QualityBadge from "../lib/components/QualityBadge.svelte";
  import Slider from "../lib/components/Slider.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { engine } from "../lib/engine/client";
  import type { LinkQuality, RouteView } from "../lib/engine/types";
  import { formatElapsed, t } from "../lib/i18n";
  import { routeIcon, routeTitle } from "../lib/routes";
  import { store } from "../lib/stores/engine.svelte";
  import { run } from "../lib/stores/toast.svelte";

  let { route }: { route: RouteView } = $props();
  let expanded = $state(false);
  let pcMuted = $state(false);

  const peer = $derived(store.state?.peers.find((p) => p.deviceId === route.peerId));
  const quality = $derived<LinkQuality>(peer?.quality ?? "unknown");
  const sending = $derived(route.kind.startsWith("send"));
  const detailsId = $props.id();
</script>

<article class="route" aria-label={routeTitle(route)}>
  <div class="main">
    <span class="icon" class:live={route.status === "active"}><Icon name={routeIcon(route.kind)} /></span>
    <div class="text">
      <strong>{routeTitle(route)}</strong>
      <span class="caption">
        {#if route.status === "active"}
          <span class="elapsed">{formatElapsed(route.elapsedSecs)}</span>
          &nbsp;<QualityBadge {quality} latencyMs={route.stats.latencyMs} />
        {:else}
          {t(`route.status.${route.status}`, route.peerName)}
        {/if}
      </span>
    </div>
    <Button
      variant="ghost"
      icon="mute"
      label={route.muted ? t("route.unmute") : t("route.mute")}
      onclick={() => run(engine.setRouteMuted(route.routeId, !route.muted))}
    />
    <Button
      variant="ghost"
      icon="chevron"
      label={t("route.details")}
      expanded={expanded}
      controls={detailsId}
      onclick={() => (expanded = !expanded)}
    />
    <Button variant="secondary" icon="stop" onclick={() => run(engine.stopRoute(route.routeId))}>{t("route.stop")}</Button>
  </div>

  {#if expanded}
    <div class="details" id={detailsId}>
      {#if !route.kind.includes("Mic")}
        <div class="row">
          <span class="grow">{t("audio.volume")}</span>
          <Slider
            value={Math.round(route.volume * 100)}
            min={0}
            max={200}
            step={5}
            label={t("audio.volume")}
            format={(v) => `${v}%`}
            onchange={(v) => run(engine.setRouteVolume(route.routeId, v / 100))}
          />
        </div>
      {/if}
      {#if sending && route.kind === "sendSystemAudio"}
        <div class="row">
          <span class="grow">{t("route.mutePc")}</span>
          <Toggle
            checked={pcMuted}
            label={t("route.mutePc")}
            onchange={(v) => {
              pcMuted = v;
              void run(engine.setPeerSpeakersMuted(store.state!.local.deviceId, v));
            }}
          />
        </div>
      {/if}
      <div class="row">
        <span class="grow">{t("route.keepRunning")}</span>
        <Toggle
          checked={route.keepRunning}
          label={t("route.keepRunning")}
          onchange={(v) => run(engine.setRouteKeepRunning(route.routeId, v))}
        />
      </div>
      <dl class="stats caption">
        <div><dt>{t("stats.codec")}</dt><dd>{route.stats.codec}{route.stats.bitrateKbps ? ` · ${route.stats.bitrateKbps} kb/s` : ""}</dd></div>
        <div><dt>{t("audio.latency")}</dt><dd>{Math.round(route.stats.latencyMs)} ms</dd></div>
        <div><dt>{t("stats.buffer")}</dt><dd>{Math.round(route.stats.bufferMs)} ms</dd></div>
        <div><dt>{t("stats.jitter")}</dt><dd>{route.stats.jitterMs.toFixed(1)} ms</dd></div>
        <div><dt>{t("stats.loss")}</dt><dd>{route.stats.lossPct.toFixed(1)} %</dd></div>
        <div><dt>{t("stats.drift")}</dt><dd>{route.stats.driftPpm} ppm</dd></div>
        <div><dt>{t("stats.dropouts")}</dt><dd>{route.stats.underruns}</dd></div>
        <div><dt>{t("stats.rtt")}</dt><dd>{peer ? Math.round(peer.rttMs) : 0} ms</dd></div>
      </dl>
    </div>
  {/if}
</article>

<style>
  .route {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    background: var(--color-surface);
  }
  .main {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: 12px var(--space-md);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: var(--color-surface-muted);
    color: var(--color-text-secondary);
  }
  .icon.live {
    color: var(--color-accent);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .elapsed {
    font-variant-numeric: tabular-nums;
  }
  .details {
    border-top: 1px solid var(--color-border);
    padding: 12px var(--space-md);
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .grow {
    flex: 1;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px 16px;
    margin: 4px 0 0;
  }
  .stats dt {
    color: var(--color-text-secondary);
  }
  .stats dd {
    margin: 0;
    color: var(--color-text-primary);
    font-variant-numeric: tabular-nums;
  }
</style>
