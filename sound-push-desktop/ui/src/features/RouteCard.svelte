<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import LatencyChart from "../lib/components/LatencyChart.svelte";
  import QualityBadge from "../lib/components/QualityBadge.svelte";
  import Slider from "../lib/components/Slider.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { connectionPath, latencyStages } from "../lib/connection";
  import { isConnected } from "../lib/devices";
  import { engine } from "../lib/engine/client";
  import type { LinkQuality, RouteView } from "../lib/engine/types";
  import { formatElapsed, formatList, t } from "../lib/i18n";
  import { routeIcon, routeTitle } from "../lib/routes";
  import { store } from "../lib/stores/engine.svelte";
  import { history } from "../lib/stores/history.svelte";
  import { platform } from "../lib/stores/platform.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run } from "../lib/stores/toast.svelte";

  let { route }: { route: RouteView } = $props();
  let expanded = $state(false);

  const peer = $derived(store.state?.peers.find((p) => p.deviceId === route.peerId));
  const quality = $derived<LinkQuality>(peer?.quality ?? "unknown");
  const detailsId = $props.id();

  // Connection details (plan §23.3, §28.2): path, where the delay comes from, recent history.
  const path = $derived(connectionPath(peer, platform.tethering?.peers.includes(route.peerId) ?? false));
  const pathText = $derived(
    [
      path.transport ? t(`path.transport.${path.transport}`) : "",
      path.kind ? t(`path.kind.${path.kind}`) : "",
      path.family ? t("path.family", path.family) : "",
    ]
      .filter(Boolean)
      .join(" · ") || "—",
  );
  const stages = $derived(latencyStages(route.stats));
  const stagesText = $derived(formatList(stages.map((s) => t("stats.stageValue", t(`stats.stage.${s.id}`), Math.round(s.ms)))));
  const samples = $derived(history.series[route.routeId] ?? []);
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
      {#if route.kind === "sendSystemAudio"}
        <!-- Sending this computer's sound: its own speakers are the ones to mute. -->
        <div class="row">
          <span class="grow">{t("audio.muteLocal")}</span>
          <Toggle
            checked={store.state?.settings.capture.muteLocalSpeakers ?? false}
            label={t("audio.muteLocal")}
            onchange={(v) => updateSettings((x) => (x.capture.muteLocalSpeakers = v))}
          />
        </div>
      {:else if route.kind === "receiveSystemAudio" && peer && isConnected(peer.connection)}
        <!-- Another computer's sound plays here: mute that computer's speakers. -->
        <div class="row">
          <span class="grow">{t("route.mutePeer", route.peerName)}</span>
          <Toggle
            checked={peer.speakersMuted}
            label={t("route.mutePeer", route.peerName)}
            onchange={(v) => run(engine.setPeerSpeakersMuted(route.peerId, v))}
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
        <div class="wide"><dt>{t("stats.path")}</dt><dd>{pathText}</dd></div>
        <div class="wide"><dt>{t("stats.address")}</dt><dd class="address">{peer?.remoteAddress || "—"}</dd></div>
        <div><dt>{t("stats.codec")}</dt><dd>{route.stats.codec}{route.stats.bitrateKbps ? ` · ${route.stats.bitrateKbps} kb/s` : ""}</dd></div>
        <div><dt>{t("audio.latency")}</dt><dd>{Math.round(route.stats.latencyMs)} ms</dd></div>
        <div><dt>{t("stats.buffer")}</dt><dd>{Math.round(route.stats.bufferMs)} ms</dd></div>
        <div><dt>{t("stats.jitter")}</dt><dd>{route.stats.jitterMs.toFixed(1)} ms</dd></div>
        <div><dt>{t("stats.loss")}</dt><dd>{route.stats.lossPct.toFixed(1)} %</dd></div>
        <div><dt>{t("stats.drift")}</dt><dd>{route.stats.driftPpm} ppm</dd></div>
        <div><dt>{t("stats.dropouts")}</dt><dd>{route.stats.underruns}</dd></div>
        <div><dt>{t("stats.rtt")}</dt><dd>{peer ? Math.round(peer.rttMs) : 0} ms</dd></div>
      </dl>
      {#if stages.length > 0}
        <div class="stages">
          <span class="caption">{t("stats.breakdown")}</span>
          <div class="bar" role="img" aria-label={stagesText}>
            {#each stages as stage (stage.id)}
              <span class="segment {stage.id}" style:flex-grow={stage.ms}></span>
            {/each}
          </div>
          <ul class="legend caption" aria-hidden="true">
            {#each stages as stage (stage.id)}
              <li><span class="swatch {stage.id}"></span>{t("stats.stageValue", t(`stats.stage.${stage.id}`), Math.round(stage.ms))}</li>
            {/each}
          </ul>
        </div>
      {/if}
      {#if route.status === "active"}<LatencyChart {samples} />{/if}
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
  .stats .wide {
    grid-column: span 2;
  }
  .stats dt {
    color: var(--color-text-secondary);
  }
  .stats dd {
    margin: 0;
    color: var(--color-text-primary);
    font-variant-numeric: tabular-nums;
  }
  .address {
    user-select: text;
    word-break: break-all;
  }
  .stages {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bar {
    display: flex;
    height: 10px;
    border-radius: 5px;
    overflow: hidden;
    gap: 2px;
  }
  .segment,
  .swatch {
    background: var(--color-accent);
  }
  .segment {
    min-width: 3px;
  }
  .capture {
    opacity: 0.35;
  }
  .encode {
    opacity: 0.5;
  }
  .network {
    opacity: 0.7;
  }
  .buffer {
    opacity: 1;
  }
  .output {
    background: var(--color-text-secondary);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
  }
  @media (forced-colors: active) {
    .segment,
    .swatch {
      background: CanvasText;
      forced-color-adjust: none;
    }
  }
</style>
