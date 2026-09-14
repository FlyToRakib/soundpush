<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Card from "../lib/components/Card.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import QualityBadge from "../lib/components/QualityBadge.svelte";
  import Select from "../lib/components/Select.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { engine, type UsbStatus } from "../lib/engine/client";
  import type {
    DeviceProfile,
    LatencyMode,
    PeerView,
    PermissionKind,
    Permissions,
    Policy,
    QualityMode,
    Recommendation,
  } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { platformIcon } from "../lib/routes";
  import { isConnected } from "../lib/devices";
  import { store } from "../lib/stores/engine.svelte";
  import { run, toasts } from "../lib/stores/toast.svelte";
  import PairDialog from "./PairDialog.svelte";

  let selectedId = $state<string | null>(null);
  let pairing = $state(false);

  const selected = $derived<PeerView | undefined>(
    store.state?.peers.find((p) => p.deviceId === selectedId) ?? store.trustedPeers[0],
  );

  const permissionRows: { kind: PermissionKind; field: keyof Permissions }[] = [
    { kind: "receiveMyAudio", field: "receive_my_audio" },
    { kind: "useMyMicrophone", field: "use_my_microphone" },
    { kind: "sendAudioToMe", field: "send_audio_to_me" },
    { kind: "controlMe", field: "control_me" },
  ];
  const policyOptions = (["allow", "ask", "deny"] as Policy[]).map((p) => ({ value: p, label: t(`policy.${p}`) }));

  // Per-device audio profile: "" in a select means "follow the Audio page".
  const stream = $derived(store.state?.settings.stream);
  const profile = $derived<DeviceProfile>(
    (selected && store.state?.settings.deviceProfiles?.[selected.deviceId]) || {},
  );
  const defaultLabel = (value: string) => `${t("devices.profile.default")} (${value})`;
  const latencyOptions = $derived([
    { value: "", label: defaultLabel(stream ? t(`latency.${stream.latency}`) : "") },
    ...(["lowLatency", "balanced", "stable"] as LatencyMode[]).map((v) => ({ value: v, label: t(`latency.${v}`) })),
  ]);
  const qualityOptions = $derived([
    { value: "", label: defaultLabel(stream ? t(`quality.mode.${stream.quality}`) : "") },
    ...(["auto", "opus", "lossless"] as QualityMode[]).map((v) => ({ value: v, label: t(`quality.mode.${v}`) })),
  ]);
  const bitrateOptions = [32000, 64000, 96000, 128000, 192000, 256000, 320000, 510000].map((b) => ({
    value: String(b),
    label: `${b / 1000} kb/s`,
  }));
  const redundancyOptions = $derived([
    { value: "", label: defaultLabel(t(stream?.redundancy ? "devices.profile.on" : "devices.profile.off")) },
    { value: "on", label: t("devices.profile.on") },
    { value: "off", label: t("devices.profile.off") },
  ]);

  function setProfile(patch: Partial<DeviceProfile>) {
    if (!selected) return;
    const next: DeviceProfile = { ...profile, ...patch };
    for (const key of Object.keys(next) as (keyof DeviceProfile)[]) {
      if (next[key] === undefined) delete next[key];
    }
    run(engine.setDeviceProfile(selected.deviceId, Object.keys(next).length > 0 ? next : null));
  }

  // Network self-test: progress and the last result come from the engine state.
  const test = $derived(store.state?.networkTests.find((n) => n.peerId === selected?.deviceId));
  const report = $derived(test?.report ?? null);

  function recommendationText(rec: Recommendation): string {
    const quality = rec.quality === "opus" ? `${rec.opusBitrate / 1000} kb/s` : t(`quality.mode.${rec.quality}`);
    const extra = rec.redundancy ? `, ${t("nettest.redundancyOn")}` : "";
    return t("nettest.recommended", t(`latency.${rec.latency}`), quality) + extra;
  }

  function applyRecommendation(rec: Recommendation) {
    setProfile({
      latency: rec.latency,
      quality: rec.quality,
      opusBitrate: rec.quality === "opus" ? rec.opusBitrate : undefined,
      redundancy: rec.redundancy,
    });
  }

  // USB via adb (desktop only: the engine listens for the forwarded connection).
  let usb = $state<UsbStatus | null>(null);
  let usbChecking = $state(false);

  async function checkUsb() {
    usbChecking = true;
    usb = (await run(engine.usbStatus())) ?? null;
    usbChecking = false;
  }

  async function useUsb(serial: string, model: string) {
    const done = await run(engine.usbConnect(serial).then(() => true));
    if (done) toasts.show(t("devices.usb.connected", model));
  }

  // Native confirm() is unavailable in the Tauri webview (it returns false immediately),
  // so confirmation uses the app's own dialog.
  let forgetting = $state<PeerView | null>(null);

  function forget(peer: PeerView) {
    forgetting = peer;
  }

  async function confirmForget() {
    const peer = forgetting;
    forgetting = null;
    if (!peer) return;
    selectedId = null;
    await run(engine.forget(peer.deviceId));
  }
</script>

<div class="layout">
  <div class="list stack">
    <div class="row">
      <h1>{t("devices.title")}</h1>
      <div class="spacer"></div>
      <Button variant="primary" icon="plus" onclick={() => (pairing = true)}>{t("devices.pair")}</Button>
    </div>

    <h2 class="caption">{t("devices.paired")}</h2>
    {#if store.trustedPeers.length === 0}
      <p class="muted">{t("devices.none")}</p>
    {/if}
    {#each store.trustedPeers as peer (peer.deviceId)}
      <button
        class="peer"
        aria-current={selected?.deviceId === peer.deviceId ? "true" : undefined}
        onclick={() => (selectedId = peer.deviceId)}
      >
        <Icon name={platformIcon(peer.platform)} />
        <span class="name">
          {peer.name}
          <span class="caption">
            {peer.blocked ? t("devices.block") : t(`status.${peer.connection}`)}{peer.transport === "tcp"
              ? ` · ${t("devices.transport.usb")}`
              : ""}
          </span>
        </span>
        <QualityBadge quality={peer.quality} />
      </button>
    {/each}

    <h2 class="caption">{t("devices.nearby")}</h2>
    {#if store.nearbyUntrusted.length === 0}
      <p class="caption">{t("devices.nearbyNone")}</p>
    {/if}
    {#each store.nearbyUntrusted as peer (peer.deviceId)}
      <div class="peer static">
        <Icon name={platformIcon(peer.platform)} />
        <span class="name">{peer.name}</span>
        <Button variant="secondary" onclick={() => run(engine.pairWithDevice(peer.deviceId))}>{t("devices.pair")}</Button>
      </div>
    {/each}

    {#if store.state && store.state.local.platform !== "android"}
      <h2 class="caption">{t("devices.usb")}</h2>
      <p class="caption">{t("devices.usb.desc")}</p>
      {#if store.state.local.tcpPort === 0}
        <p class="caption">{t("devices.usb.unavailable")}</p>
      {:else}
        {#if usb && !usb.adbFound}
          <p class="caption">{t("devices.usb.noAdb")}</p>
        {:else if usb && usb.devices.length === 0}
          <p class="caption">{t("devices.usb.noDevice")}</p>
        {/if}
        {#each usb?.devices ?? [] as device (device.serial)}
          {#if device.authorized}
            <Button variant="secondary" onclick={() => useUsb(device.serial, device.model)}>
              {t("devices.usb.connect", device.model)}
            </Button>
          {:else}
            <p class="caption">{t("devices.usb.unauthorized", device.model)}</p>
          {/if}
        {/each}
        <div class="row">
          <Button variant="ghost" disabled={usbChecking} onclick={checkUsb}>{t("devices.usb.check")}</Button>
        </div>
      {/if}
    {/if}
  </div>

  {#if selected && selected.trusted}
    <div class="detail stack">
      <div class="row">
        <Icon name={platformIcon(selected.platform)} size={28} />
        <div class="stack tight">
          <h1>{selected.name}</h1>
          <span class="caption"
            >{t(`status.${selected.connection}`)}{selected.transport === "tcp"
              ? ` · ${t("devices.transport.usb")}`
              : ""}{selected.rttMs ? ` · ${Math.round(selected.rttMs)} ms` : ""}</span
          >
        </div>
        <div class="spacer"></div>
        {#if isConnected(selected.connection)}
          <Button onclick={() => run(engine.disconnect(selected.deviceId))}>{t("devices.disconnect")}</Button>
        {:else}
          <Button variant="primary" onclick={() => run(engine.connect(selected.deviceId))}>{t("devices.connect")}</Button>
        {/if}
      </div>

      <Card>
        <SettingRow label={t("devices.rename")} id="rename">
          <input
            id="rename"
            class="text-input"
            value={selected.name}
            onchange={(e) => run(engine.rename(selected.deviceId, (e.currentTarget as HTMLInputElement).value || null))}
          />
        </SettingRow>
        <SettingRow label={t("devices.autoConnect")}>
          <Toggle
            checked={selected.autoConnect}
            label={t("devices.autoConnect")}
            onchange={(v) => run(engine.setAutoConnect(selected.deviceId, v))}
          />
        </SettingRow>
      </Card>

      {#if selected.permissions}
        <Card title={t("devices.permissions")}>
          {#each permissionRows as row (row.kind)}
            <SettingRow label={t(`perm.${row.kind}`)}>
              <Select
                value={selected.permissions[row.field]}
                options={policyOptions}
                label={t(`perm.${row.kind}`)}
                onchange={(v) => run(engine.setPermission(selected.deviceId, row.kind, v as Policy))}
              />
            </SettingRow>
          {/each}
        </Card>
      {/if}

      <Card title={t("devices.profile")} description={t("devices.profile.desc")}>
        <SettingRow label={t("devices.profile.latency")}>
          <Select
            value={profile.latency ?? ""}
            options={latencyOptions}
            label={t("devices.profile.latency")}
            onchange={(v) => setProfile({ latency: (v || undefined) as LatencyMode | undefined })}
          />
        </SettingRow>
        <SettingRow label={t("devices.profile.quality")}>
          <Select
            value={profile.quality ?? ""}
            options={qualityOptions}
            label={t("devices.profile.quality")}
            onchange={(v) =>
              setProfile({
                quality: (v || undefined) as QualityMode | undefined,
                opusBitrate: v === "opus" ? (profile.opusBitrate ?? stream?.opusBitrate) : undefined,
              })}
          />
        </SettingRow>
        {#if profile.quality === "opus"}
          <SettingRow label={t("devices.profile.bitrate")}>
            <Select
              value={String(profile.opusBitrate ?? stream?.opusBitrate ?? 128000)}
              options={bitrateOptions}
              label={t("devices.profile.bitrate")}
              onchange={(v) => setProfile({ opusBitrate: Number(v) })}
            />
          </SettingRow>
        {/if}
        <SettingRow label={t("devices.profile.redundancy")}>
          <Select
            value={profile.redundancy === undefined ? "" : profile.redundancy ? "on" : "off"}
            options={redundancyOptions}
            label={t("devices.profile.redundancy")}
            onchange={(v) => setProfile({ redundancy: v === "" ? undefined : v === "on" })}
          />
        </SettingRow>
      </Card>

      <Card title={t("nettest.title")} description={t("nettest.desc")}>
        <div class="row test-actions">
          {#if test?.status === "running"}
            <span role="status">{t("nettest.running", Math.round(test.progress * 100))}</span>
            <div class="spacer"></div>
            <Button onclick={() => run(engine.cancelNetworkTest(selected.deviceId))}>{t("nettest.cancel")}</Button>
          {:else}
            <Button
              variant="primary"
              disabled={!isConnected(selected.connection)}
              onclick={() => run(engine.runNetworkTest(selected.deviceId))}>{t("nettest.run")}</Button
            >
          {/if}
        </div>
        {#if test?.status === "failed"}
          <p class="muted" role="status">{t("nettest.failed")}</p>
        {/if}
        {#if report && test?.status !== "running"}
          <dl class="results">
            <div><dt>{t("nettest.rtt")}</dt><dd>{t("nettest.ms", report.rttMs.toFixed(1))}</dd></div>
            <div><dt>{t("nettest.jitter")}</dt><dd>{t("nettest.ms", report.jitterMs.toFixed(1))}</dd></div>
            <div><dt>{t("nettest.loss")}</dt><dd>{t("nettest.pct", report.lossPct.toFixed(1))}</dd></div>
            <div><dt>{t("nettest.bitrate")}</dt><dd>{t("nettest.kbps", report.achievableKbps)}</dd></div>
          </dl>
          {#each report.recommendation.tips as tip (tip)}
            <p class="muted">{t(tip)}</p>
          {/each}
          <div class="row test-actions">
            <span class="caption">{recommendationText(report.recommendation)}</span>
            <div class="spacer"></div>
            <Button onclick={() => applyRecommendation(report.recommendation)}>{t("nettest.apply")}</Button>
          </div>
        {/if}
      </Card>

      <div class="row">
        <Button variant="secondary" onclick={() => run(engine.setBlocked(selected.deviceId, !selected.blocked))}>
          {selected.blocked ? t("devices.unblock") : t("devices.block")}
        </Button>
        <Button variant="danger" onclick={() => forget(selected)}>{t("devices.forget")}</Button>
      </div>
    </div>
  {/if}
</div>

{#if pairing}<PairDialog onclose={() => (pairing = false)} />{/if}

{#if forgetting}
  <Dialog title={t("devices.forget")} onclose={() => (forgetting = null)}>
    <p class="muted">{t("devices.forget.confirm", forgetting.name)}</p>
    {#snippet actions()}
      <Button onclick={() => (forgetting = null)}>{t("common.cancel")}</Button>
      <Button variant="danger" onclick={confirmForget}>{t("devices.forget")}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-xl);
    max-width: 1000px;
  }
  .layout > :global(*) {
    min-width: 0;
  }
  /* Side-by-side list and details only when there is room; stacked on narrow windows. */
  @media (min-width: 980px) {
    .layout {
      grid-template-columns: minmax(240px, 300px) minmax(0, 1fr);
    }
  }
  .list {
    gap: var(--space-sm);
  }
  .list h2 {
    margin-top: var(--space-md);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .peer {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: 10px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: transparent;
    cursor: pointer;
    text-align: start;
  }
  .peer:hover,
  .peer[aria-current="true"] {
    background: var(--color-surface);
    border-color: var(--color-border);
  }
  .peer.static {
    cursor: default;
  }
  .name {
    flex: 1;
    display: flex;
    flex-direction: column;
    font-weight: 500;
  }
  .tight {
    gap: 0;
  }
  .text-input {
    height: 34px;
    width: 220px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-background);
    user-select: text;
  }
  .test-actions {
    padding: var(--space-sm) 0;
  }
  .results {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
    gap: var(--space-sm);
    margin: 0 0 var(--space-sm);
  }
  .results dt {
    color: var(--color-text-muted);
    font-size: 0.85em;
  }
  .results dd {
    margin: 0;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
</style>
