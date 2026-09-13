<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Card from "../lib/components/Card.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import QualityBadge from "../lib/components/QualityBadge.svelte";
  import Select from "../lib/components/Select.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { engine } from "../lib/engine/client";
  import type { PeerView, PermissionKind, Permissions, Policy } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { platformIcon } from "../lib/routes";
  import { store } from "../lib/stores/engine.svelte";
  import { run } from "../lib/stores/toast.svelte";
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
          <span class="caption">{peer.blocked ? t("devices.block") : t(`status.${peer.connection}`)}</span>
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
  </div>

  {#if selected && selected.trusted}
    <div class="detail stack">
      <div class="row">
        <Icon name={platformIcon(selected.platform)} size={28} />
        <div class="stack tight">
          <h1>{selected.name}</h1>
          <span class="caption">{t(`status.${selected.connection}`)}{selected.rttMs ? ` · ${Math.round(selected.rttMs)} ms` : ""}</span>
        </div>
        <div class="spacer"></div>
        {#if selected.connection === "connected"}
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
    text-align: left;
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
</style>
