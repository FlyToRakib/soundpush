<script lang="ts">
  import Card from "../lib/components/Card.svelte";
  import LevelMeter from "../lib/components/LevelMeter.svelte";
  import Segmented from "../lib/components/Segmented.svelte";
  import Select from "../lib/components/Select.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import Slider from "../lib/components/Slider.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import type { LatencyMode, QualityMode } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { updateSettings } from "../lib/stores/settings";

  const app = $derived(store.state!);
  const s = $derived(app.settings);
  let advanced = $state(false);

  const DEFAULT = "__default__";
  const outputs = $derived([
    { value: DEFAULT, label: t("audio.followDefault") },
    ...app.audioDevices.filter((d) => !d.isInput).map((d) => ({ value: d.id, label: d.name })),
  ]);
  const inputs = $derived([
    { value: DEFAULT, label: t("audio.followDefault") },
    ...app.audioDevices.filter((d) => d.isInput).map((d) => ({ value: d.id, label: d.name })),
  ]);
  const virtualTargets = $derived([
    { value: DEFAULT, label: t("audio.virtualMic.none") },
    ...app.audioDevices.filter((d) => !d.isInput).map((d) => ({ value: d.id, label: d.name })),
  ]);
  const bitrates = [10, 24, 32, 64, 96, 128, 192, 256, 320, 450, 510].map((k) => ({
    value: String(k * 1000),
    label: `${k} kb/s`,
  }));
  const fromSelect = (v: string) => (v === DEFAULT ? null : v);
</script>

<div class="page stack">
  <h1>{t("audio.title")}</h1>

  <Card title={t("audio.latency")} description={t(`latency.${s.stream.latency}.desc`)}>
    <Segmented
      value={s.stream.latency}
      label={t("audio.latency")}
      options={(["lowLatency", "balanced", "stable", "custom"] as LatencyMode[]).map((v) => ({ value: v, label: t(`latency.${v}`) }))}
      onchange={(v) => updateSettings((x) => (x.stream.latency = v))}
    />
    {#if s.stream.latency === "custom"}
      <SettingRow label={t("latency.min")}>
        <Slider value={s.stream.customMinMs} min={5} max={500} step={5} label={t("latency.min")} format={(v) => `${v} ms`}
          onchange={(v) => updateSettings((x) => (x.stream.customMinMs = v))} />
      </SettingRow>
      <SettingRow label={t("latency.max")}>
        <Slider value={s.stream.customMaxMs} min={s.stream.customMinMs} max={1000} step={5} label={t("latency.max")} format={(v) => `${v} ms`}
          onchange={(v) => updateSettings((x) => (x.stream.customMaxMs = v))} />
      </SettingRow>
    {/if}
    <SettingRow label={t("audio.quality")}>
      <Select
        value={s.stream.quality}
        label={t("audio.quality")}
        options={(["auto", "opus", "lossless"] as QualityMode[]).map((v) => ({ value: v, label: t(`quality.mode.${v}`) }))}
        onchange={(v) => updateSettings((x) => (x.stream.quality = v as QualityMode))}
      />
    </SettingRow>
    {#if s.stream.quality === "opus"}
      <SettingRow label={t("quality.bitrate")}>
        <Select value={String(s.stream.opusBitrate)} label={t("quality.bitrate")} options={bitrates}
          onchange={(v) => updateSettings((x) => (x.stream.opusBitrate = Number(v)))} />
      </SettingRow>
    {/if}
  </Card>

  <Card title={t("audio.virtualMic")} description={t("audio.virtualMic.desc")}>
    <SettingRow label={t("audio.virtualMic.device")} description={t("audio.virtualMic.help")}>
      <Select
        value={s.desktop.virtualMicDevice ?? DEFAULT}
        label={t("audio.virtualMic.device")}
        options={virtualTargets}
        onchange={(v) => updateSettings((x) => (x.desktop.virtualMicDevice = fromSelect(v)))}
      />
    </SettingRow>
  </Card>

  <Card title={t("audio.send")}>
    <SettingRow label={t("audio.systemSource")}>
      <Select value={s.capture.systemDevice ?? DEFAULT} label={t("audio.systemSource")} options={outputs}
        onchange={(v) => updateSettings((x) => (x.capture.systemDevice = fromSelect(v)))} />
    </SettingRow>
    <SettingRow label={t("audio.muteLocal")}>
      <Toggle checked={s.capture.muteLocalSpeakers} label={t("audio.muteLocal")}
        onchange={(v) => updateSettings((x) => (x.capture.muteLocalSpeakers = v))} />
    </SettingRow>
  </Card>

  <Card title={t("audio.mic")}>
    <SettingRow label={t("audio.micDevice")}>
      <Select value={s.mic.device ?? DEFAULT} label={t("audio.micDevice")} options={inputs}
        onchange={(v) => updateSettings((x) => (x.mic.device = fromSelect(v)))} />
    </SettingRow>
    <SettingRow label={t("audio.gain")}>
      <Slider value={s.mic.gainDb} min={0} max={20} step={1} label={t("audio.gain")} format={(v) => `+${v} dB`}
        onchange={(v) => updateSettings((x) => (x.mic.gainDb = v))} />
    </SettingRow>
    <SettingRow label={t("audio.noiseSuppression")} description={t("audio.noiseSuppression.desc")}>
      <Toggle checked={s.mic.noiseSuppression} label={t("audio.noiseSuppression")}
        onchange={(v) => updateSettings((x) => (x.mic.noiseSuppression = v))} />
    </SettingRow>
    <SettingRow label={t("audio.monitor")} description={t("audio.monitor.desc")}>
      {#if s.mic.monitor}<LevelMeter db={app.micLevelDb} label={t("audio.mic")} />{/if}
      <Toggle checked={s.mic.monitor} label={t("audio.monitor")} onchange={(v) => updateSettings((x) => (x.mic.monitor = v))} />
    </SettingRow>
  </Card>

  <Card title={t("audio.playback")}>
    <SettingRow label={t("audio.output")}>
      <Select value={s.output.device ?? DEFAULT} label={t("audio.output")} options={outputs}
        onchange={(v) => updateSettings((x) => (x.output.device = fromSelect(v)))} />
    </SettingRow>
    <SettingRow label={t("audio.volume")}>
      <Slider value={Math.round(s.output.volume * 100)} min={0} max={200} step={5} label={t("audio.volume")} format={(v) => `${v}%`}
        onchange={(v) => updateSettings((x) => (x.output.volume = v / 100))} />
    </SettingRow>
    <button class="disclosure" aria-expanded={advanced} onclick={() => (advanced = !advanced)}>{t("common.advanced")}</button>
    {#if advanced}
      <SettingRow label={t("audio.balance")}>
        <Slider value={Math.round(s.output.balance * 100)} min={-100} max={100} step={5} label={t("audio.balance")}
          format={(v) => (v === 0 ? "C" : v < 0 ? `L${-v}` : `R${v}`)}
          onchange={(v) => updateSettings((x) => (x.output.balance = v / 100))} />
      </SettingRow>
      <SettingRow label={t("audio.mono")}>
        <Toggle checked={s.output.mono} label={t("audio.mono")} onchange={(v) => updateSettings((x) => (x.output.mono = v))} />
      </SettingRow>
      <SettingRow label={t("audio.avOffset")}>
        <Slider value={s.output.avOffsetMs} min={0} max={500} step={10} label={t("audio.avOffset")} format={(v) => `${v} ms`}
          onchange={(v) => updateSettings((x) => (x.output.avOffsetMs = v))} />
      </SettingRow>
    {/if}
  </Card>
</div>

<style>
  .page {
    max-width: 760px;
  }
  .disclosure {
    align-self: flex-start;
    border: 0;
    background: transparent;
    color: var(--color-accent);
    cursor: pointer;
    padding: 0;
  }
</style>
