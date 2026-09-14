<script lang="ts">
  import { onMount } from "svelte";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Card from "../lib/components/Card.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import HotkeyInput from "../lib/components/HotkeyInput.svelte";
  import { DEFAULT_DEVICE, deviceOptions, isBluetoothOutput } from "../lib/devices";
  import { engine, type VirtualMicStatus } from "../lib/engine/client";
  import { platform } from "../lib/stores/platform.svelte";
  import { run, toasts } from "../lib/stores/toast.svelte";
  import LevelMeter from "../lib/components/LevelMeter.svelte";
  import Segmented from "../lib/components/Segmented.svelte";
  import Select from "../lib/components/Select.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import Slider from "../lib/components/Slider.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import type { AudioApps, HotkeyKind, LatencyMode, QualityMode } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { updateSettings } from "../lib/stores/settings";

  const app = $derived(store.state!);
  const s = $derived(app.settings);
  let advanced = $state(false);

  const DEFAULT = DEFAULT_DEVICE;
  const unavailable = (name: string) => t("audio.deviceUnavailable", name);
  // Virtual cables are microphone plumbing, not speakers.
  const speakers = $derived(app.audioDevices.filter((d) => !d.isInput && !d.virtualCable));
  const sourceOptions = $derived(deviceOptions(speakers, s.capture.systemDevice, t("audio.followDefault"), unavailable));
  const outputOptions = $derived(deviceOptions(speakers, s.output.device, t("audio.followDefault"), unavailable));
  const inputs = $derived(
    deviceOptions(
      app.audioDevices.filter((d) => d.isInput),
      s.mic.device,
      t("audio.followDefault"),
      unavailable,
    ),
  );
  const isMac = $derived(app.local.platform === "macos");
  const bluetoothOutput = $derived(
    platform.system !== null && isBluetoothOutput(s.output.device, platform.system.defaultOutput, platform.system.bluetoothOutputs),
  );
  const micPermission = $derived(platform.system?.microphone ?? "unknown");

  /** Apps playing audio, for sending one app (Windows 10 2004+). */
  const ALL_APPS = "__all__";
  let apps = $state<AudioApps | null>(null);
  const appName = (process: string) => process.replace(/\.exe$/i, "");
  const appOptions = $derived([
    { value: ALL_APPS, label: t("audio.apps.all") },
    ...(apps?.apps ?? []).map((a) => ({ value: a.process, label: appName(a.process) })),
    ...(s.capture.app && !apps?.apps.some((a) => a.process.toLowerCase() === s.capture.app?.toLowerCase())
      ? [{ value: s.capture.app, label: t("audio.apps.notRunning", appName(s.capture.app)) }]
      : []),
  ]);

  async function loadApps() {
    try {
      apps = await engine.listAudioApps();
    } catch {
      apps = null;
    }
  }

  const hotkeyRows = $derived([
    { kind: "mute" as HotkeyKind, label: "audio.muteHotkey", value: s.desktop.muteHotkey },
    { kind: "pushToTalk" as HotkeyKind, label: "audio.pushToTalkHotkey", value: s.desktop.pushToTalkHotkey },
  ]);
  let hotkeyErrors = $state<Record<HotkeyKind, string | null>>({ mute: null, pushToTalk: null });

  function hotkeyDescription(kind: HotkeyKind): string {
    const registered = platform.hotkeys[kind];
    return hotkeyErrors[kind] ?? (registered ? t(`error.hotkey.${registered}`) : t(`audio.${kind}Hotkey.desc`));
  }

  async function setHotkey(kind: HotkeyKind, accelerator: string | null) {
    try {
      await engine.setHotkey(kind, accelerator);
      hotkeyErrors[kind] = null;
    } catch (e) {
      hotkeyErrors[kind] = t((e as { key?: string }).key ?? "error.hotkey.unavailable");
    }
    await platform.refresh();
  }
  // Only virtual cables can act as a microphone for other apps; real speakers would
  // just play the phone's microphone out loud.
  const cable = navigator.userAgent.includes("Mac")
    ? { name: "BlackHole", url: "https://existential.audio/blackhole/" }
    : { name: "VB-CABLE", url: "https://vb-audio.com/Cable/" };
  const chosenVm = $derived(s.desktop.virtualMicDevice);
  const chosenIsCable = $derived(app.audioDevices.some((d) => d.id === chosenVm && d.virtualCable));
  /** A chosen device SoundPush can't use (a speaker, or a cable that is no longer present). */
  const ignoredVm = $derived(chosenVm && app.capabilities.virtualMicDevice !== chosenVm ? chosenVm : null);
  const virtualTargets = $derived([
    { value: DEFAULT, label: t("audio.virtualMic.none") },
    ...app.audioDevices.filter((d) => d.virtualCable).map((d) => ({ value: d.id, label: d.name })),
    ...(chosenVm && !chosenIsCable ? [{ value: chosenVm, label: t("audio.virtualMic.notCableOption", chosenVm) }] : []),
  ]);

  /** Virtual microphone driver: our own on macOS, VB-CABLE on Windows. */
  let driver = $state<VirtualMicStatus | null>(null);
  /** i18n key prefix for the driver's texts. */
  const driverKey = $derived(driver?.provider === "vbcable" ? "audio.virtualMic.vb" : "audio.virtualMic.sp");
  let installing = $state(false);
  let confirmRestart = $state(false);

  async function loadDriver() {
    try {
      driver = await engine.virtualMicStatus();
    } catch {
      driver = null;
    }
  }

  async function recheck() {
    await loadDriver();
    await run(engine.refreshAudioDevices());
  }

  async function changeDriver(install: boolean) {
    if (installing) return;
    installing = true;
    try {
      await (install ? engine.installVirtualMic() : engine.uninstallVirtualMic());
      if (install) toasts.show(t(`${driverKey}.installed`), "info");
    } catch (e) {
      const message = (e as { message?: string }).message ?? String(e);
      // Closing the password prompt is a choice, not an error.
      if (!message.includes("cancelled")) toasts.show(t("audio.virtualMic.installFailed", message), "error");
    } finally {
      installing = false;
      await loadDriver();
    }
  }

  // Pick up virtual microphones installed while the app was running.
  onMount(() => {
    void loadDriver();
    void loadApps();
    void engine.refreshAudioDevices().catch(() => {});
  });
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
    {#if ignoredVm}
      <Banner
        severity="warning"
        message={t(app.audioDevices.some((d) => d.id === ignoredVm) ? "audio.virtualMic.notCable" : "audio.virtualMic.notFound", ignoredVm)}
        actionLabel={t("audio.virtualMic.useAuto")}
        onaction={() => updateSettings((x) => (x.desktop.virtualMicDevice = null))}
      />
    {/if}
    {#if app.capabilities.virtualMicInput}
      <p class="vm-ready">{t("audio.virtualMic.ready", app.capabilities.virtualMicInput)}</p>
      {#if app.capabilities.virtualMicInput.includes("CABLE")}
        <!-- Credit required by VB-Audio's donationware licence. -->
        <p class="caption">
          {t("audio.virtualMic.vbCredit")}
          <button class="link" onclick={() => run(engine.openUrl("https://vb-audio.com/Cable/"))}>vb-audio.com</button>
        </p>
      {/if}
      {#if driver?.installed && driver.provider === "soundpush"}
        <div class="row">
          <Button variant="ghost" onclick={() => changeDriver(false)}>{t("audio.virtualMic.remove")}</Button>
        </div>
      {/if}
    {:else if driver?.supported}
      <div class="vm-setup">
        <!-- Installed but not detected: Windows needs a restart; macOS usually just a re-check. -->
        <p class="muted">{t(`${driverKey}.${driver.installed ? "pending" : "hint"}`)}</p>
        <div class="row">
          {#if !driver.installed}
            <Button variant="primary" onclick={() => changeDriver(true)}>
              {t(installing ? "audio.virtualMic.installing" : `${driverKey}.install`)}
            </Button>
          {:else if driver.provider === "vbcable"}
            <Button variant="primary" onclick={() => (confirmRestart = true)}>{t("audio.virtualMic.restart")}</Button>
          {/if}
          <Button onclick={recheck}>{t("audio.virtualMic.recheck")}</Button>
        </div>
        {#if driver.provider === "vbcable"}<p class="caption">{t("audio.virtualMic.vbCredit")}</p>{/if}
      </div>
    {:else}
      <div class="vm-setup">
        <p class="muted">{t("audio.virtualMic.missing", cable.name)}</p>
        <div class="row">
          <Button variant="primary" onclick={() => run(engine.openUrl(cable.url))}>{t("audio.virtualMic.get", cable.name)}</Button>
          <Button onclick={() => run(engine.refreshAudioDevices())}>{t("audio.virtualMic.recheck")}</Button>
        </div>
      </div>
    {/if}
    <SettingRow label={t("audio.virtualMic.device")} description={t("audio.virtualMic.help")}>
      <Select
        value={s.desktop.virtualMicDevice ?? DEFAULT}
        label={t("audio.virtualMic.device")}
        options={virtualTargets}
        onchange={(v) => updateSettings((x) => (x.desktop.virtualMicDevice = fromSelect(v)))}
      />
    </SettingRow>
    <SettingRow label={t("audio.virtualMic.autoStart")} description={t("audio.virtualMic.autoStart.desc")}>
      <Toggle checked={s.desktop.autoStartMic} label={t("audio.virtualMic.autoStart")}
        onchange={(v) => updateSettings((x) => (x.desktop.autoStartMic = v))} />
    </SettingRow>
  </Card>

  <Card title={t("audio.send")}>
    {#if apps?.supported}
      <SettingRow label={t("audio.apps")} description={t("audio.apps.desc")}>
        <Button variant="ghost" onclick={loadApps}>{t("audio.apps.refresh")}</Button>
        <Select value={s.capture.app ?? ALL_APPS} label={t("audio.apps")} options={appOptions}
          onchange={(v) => updateSettings((x) => (x.capture.app = v === ALL_APPS ? null : v))} />
      </SettingRow>
      {#if s.capture.app}
        <SettingRow label={t("audio.apps.mode")}>
          <Segmented
            value={s.capture.excludeApp ? "exclude" : "only"}
            label={t("audio.apps.mode")}
            options={[
              { value: "only", label: t("audio.apps.only") },
              { value: "exclude", label: t("audio.apps.exclude") },
            ]}
            onchange={(v) => updateSettings((x) => (x.capture.excludeApp = v === "exclude"))}
          />
        </SettingRow>
      {/if}
    {/if}
    {#if !s.capture.app}
      <SettingRow label={t("audio.systemSource")}>
        <Select value={s.capture.systemDevice ?? DEFAULT} label={t("audio.systemSource")} options={sourceOptions}
          onchange={(v) => updateSettings((x) => (x.capture.systemDevice = fromSelect(v)))} />
      </SettingRow>
    {/if}
    <SettingRow label={t("audio.muteLocal")}>
      <Toggle checked={s.capture.muteLocalSpeakers} label={t("audio.muteLocal")}
        onchange={(v) => updateSettings((x) => (x.capture.muteLocalSpeakers = v))} />
    </SettingRow>
  </Card>

  <Card title={t("audio.mic")}>
    {#if micPermission === "denied" || micPermission === "restricted"}
      <Banner
        severity="warning"
        message={t("audio.micDenied")}
        actionLabel={t("common.openSettings")}
        onaction={() => run(engine.openSystemSettings("microphone"))}
      />
    {:else if micPermission === "notDetermined"}
      <Banner
        severity="info"
        message={t("audio.micAsk")}
        actionLabel={t("audio.micAllow")}
        onaction={async () => {
          await run(engine.requestMicrophone());
          await platform.refresh();
        }}
      />
    {/if}
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

  <Card title={t("audio.hotkeys")} description={t("audio.hotkeys.desc")}>
    {#each hotkeyRows as row (row.kind)}
      <SettingRow label={t(row.label)} description={hotkeyDescription(row.kind)}>
        <HotkeyInput value={row.value} label={t(row.label)} mac={isMac} onchange={(a) => setHotkey(row.kind, a)} />
      </SettingRow>
    {/each}
  </Card>

  <Card title={t("audio.playback")}>
    <SettingRow label={t("audio.output")}>
      <Select value={s.output.device ?? DEFAULT} label={t("audio.output")} options={outputOptions}
        onchange={(v) => updateSettings((x) => (x.output.device = fromSelect(v)))} />
    </SettingRow>
    {#if bluetoothOutput}
      <Banner
        severity="info"
        message={t("audio.bluetoothHint")}
        actionLabel={s.stream.latency === "stable" ? undefined : t("trouble.action.setStable")}
        onaction={() => updateSettings((x) => (x.stream.latency = "stable"))}
      />
    {/if}
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

{#if confirmRestart}
  <Dialog title={t("audio.virtualMic.restartTitle")} onclose={() => (confirmRestart = false)}>
    <p class="muted">{t("audio.virtualMic.restartBody")}</p>
    {#snippet actions()}
      <Button onclick={() => (confirmRestart = false)}>{t("audio.virtualMic.notNow")}</Button>
      <Button
        variant="primary"
        onclick={() => {
          confirmRestart = false;
          void run(engine.restartComputer());
        }}>{t("audio.virtualMic.restart")}</Button
      >
    {/snippet}
  </Dialog>
{/if}

<style>
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--color-accent);
    cursor: pointer;
    font: inherit;
  }
  .page {
    max-width: 760px;
  }
  .vm-ready,
  .vm-setup p {
    margin: 0;
  }
  .vm-setup {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
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
