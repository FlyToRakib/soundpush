<script lang="ts">
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import { engine } from "../lib/engine/client";
  import type { FixAction } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { run, toasts } from "../lib/stores/toast.svelte";
  import { ui } from "../lib/stores/ui.svelte";
  import FirewallFix from "./FirewallFix.svelte";

  /** What a notice's fix button does. Fixes without an entry show no button. */
  const FIXES: Partial<Record<FixAction, () => void>> = {
    openFirewallFix: () => (ui.firewallFix = true),
    installVirtualMic: () => (ui.page = "audio"),
    chooseAnotherDevice: () => (ui.page = "audio"),
    openMicPermissionSettings: () => void run(engine.openSystemSettings("microphone")),
    pairAgain: () => (ui.page = "devices"),
    switchToUsb: () => {
      ui.page = "settings";
      ui.troubleshoot = "noDevices";
    },
  };

  const app = $derived(store.state!);
  const prompt = $derived(app.pairing.prompts[0]);
  const request = $derived(app.requests[0]);
  const notices = $derived(app.notices.slice(-3));

  // Engine notices close by themselves like toasts do; nothing should need a manual close.
  // Warnings and errors stay a little longer so they can be read.
  const scheduled = new Set<number>();
  $effect(() => {
    for (const notice of notices) {
      if (scheduled.has(notice.id)) continue;
      scheduled.add(notice.id);
      const delay = notice.severity === "info" ? 4000 : 8000;
      setTimeout(() => {
        scheduled.delete(notice.id);
        void engine.dismissNotice(notice.id).catch(() => {});
      }, delay);
    }
  });

  function requestTitle(kind: string, name: string): string {
    // `kind` is from this computer's point of view: "sendMic…" = the other device wants our microphone.
    if (kind.startsWith("sendMic")) return t("request.title", name);
    if (kind.startsWith("send")) return t("request.titleAudio", name);
    return t("request.titleSend", name);
  }
</script>

{#if prompt}
  <Dialog title={t("pair.codeTitle", prompt.peerName)}>
    <p class="muted">{t("pair.codeBody")}</p>
    <p class="code" aria-label={prompt.code.split("").join(" ")}>{prompt.code.slice(0, 3)} {prompt.code.slice(3)}</p>
    {#if prompt.peerConfirmed}<p class="caption">{t("pair.peerConfirmed", prompt.peerName)}</p>{/if}
    {#snippet actions()}
      <Button onclick={() => run(engine.confirmPairing(prompt.peerId, false))}>{t("pair.codeMismatch")}</Button>
      <Button variant="primary" onclick={() => run(engine.confirmPairing(prompt.peerId, true))}>{t("pair.codeMatch")}</Button>
    {/snippet}
  </Dialog>
{:else if request}
  <Dialog title={requestTitle(request.kind, request.peerName)}>
    <p class="muted">{t(`route.${request.kind}`, request.peerName)}</p>
    {#snippet actions()}
      <Button onclick={() => run(engine.respondRouteRequest(request.requestId, false, false))}>{t("request.deny")}</Button>
      <Button onclick={() => run(engine.respondRouteRequest(request.requestId, true, true))}>{t("request.always")}</Button>
      <Button variant="primary" onclick={() => run(engine.respondRouteRequest(request.requestId, true, false))}>
        {t("request.allowOnce")}
      </Button>
    {/snippet}
  </Dialog>
{/if}

{#if ui.firewallFix}<FirewallFix onclose={() => (ui.firewallFix = false)} />{/if}

<div class="toasts" aria-live="polite">
  {#each notices as notice (notice.id)}
    {@const fix = notice.error?.fix ? FIXES[notice.error.fix] : undefined}
    <Banner
      severity={notice.severity}
      message={t(notice.key, ...notice.args)}
      actionLabel={fix && notice.error?.fix ? t(`fix.${notice.error.fix}`) : undefined}
      onaction={fix
        ? () => {
            fix();
            void run(engine.dismissNotice(notice.id));
          }
        : undefined}
      ondismiss={() => run(engine.dismissNotice(notice.id))}
    />
  {/each}
  {#each toasts.items as toast (toast.id)}
    <Banner severity={toast.severity} message={toast.message} ondismiss={() => toasts.dismiss(toast.id)} />
  {/each}
</div>

<style>
  .code {
    font-size: 36px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-align: center;
    font-variant-numeric: tabular-nums;
    padding: var(--space-sm) 0;
  }
  .toasts {
    position: fixed;
    right: var(--space-lg);
    bottom: var(--space-lg);
    width: min(380px, calc(100vw - 48px));
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    z-index: 10;
  }
  .toasts :global(.banner) {
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.12);
  }
</style>
