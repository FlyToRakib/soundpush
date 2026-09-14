<script lang="ts">
  // First run (plan §23.2): Welcome → device name and launch at sign-in (§24, asked here) →
  // pair the phone, with a code to download the Android app alongside. Skippable at every step.
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import QrCode from "../lib/components/QrCode.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { LINKS } from "../lib/links";
  import { store } from "../lib/stores/engine.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run } from "../lib/stores/toast.svelte";

  let step = $state<"welcome" | "name" | "pair">("welcome");
  let name = $state(store.state?.settings.deviceName ?? "");
  let launchAtLogin = $state(store.state?.settings.desktop.launchAtLogin ?? true);
  const startTrusted = store.trustedPeers.length;
  const launchLabel = $derived(
    store.state?.local.platform === "macos" ? t("settings.launchAtLogin.mac") : t("settings.launchAtLogin"),
  );

  function finish() {
    void engine.stopPairing();
    const answered = step !== "welcome";
    updateSettings((s) => {
      if (name.trim()) s.deviceName = name.trim();
      if (answered) s.desktop.launchAtLogin = launchAtLogin;
      if (!s.dismissedTips.includes("onboarding")) s.dismissedTips.push("onboarding");
    });
  }

  $effect(() => {
    if (step === "pair") void run(engine.startPairing());
  });
  $effect(() => {
    if (step === "pair" && store.trustedPeers.length > startTrusted) finish();
  });
</script>

<Dialog title={step === "name" ? t("onboarding.name") : step === "pair" ? t("pair.title") : t("onboarding.welcome")}>
  {#if step === "welcome"}
    <p class="muted">{t("onboarding.welcome.body")}</p>
  {:else if step === "name"}
    <label class="sr-only" for="onboarding-name">{t("settings.deviceName")}</label>
    <input id="onboarding-name" bind:value={name} maxlength="64" />
    <p class="caption">{t("settings.deviceName.desc")}</p>
    <div class="launch">
      <span class="grow">
        <span class="label">{launchLabel}</span>
        <span class="caption">{t("onboarding.launchAtLogin.desc")}</span>
      </span>
      <Toggle checked={launchAtLogin} label={launchLabel} onchange={(v) => (launchAtLogin = v)} />
    </div>
  {:else}
    <p class="muted">{t("pair.scan")}</p>
    {#if store.state?.pairing.qrUri}<QrCode value={store.state.pairing.qrUri} label={t("pair.title")} />{/if}
    <div class="get-app">
      <QrCode value={LINKS.androidApp} label={t("onboarding.getApp.qr")} size={104} />
      <p class="caption">{t("onboarding.getApp")}</p>
    </div>
  {/if}

  {#snippet actions()}
    <Button variant="ghost" onclick={finish}>{t("onboarding.skip")}</Button>
    {#if step === "welcome"}
      <Button variant="primary" onclick={() => (step = "name")}>{t("onboarding.next")}</Button>
    {:else if step === "name"}
      <Button
        variant="primary"
        disabled={!name.trim()}
        onclick={() => {
          updateSettings((s) => {
            s.deviceName = name.trim();
            s.desktop.launchAtLogin = launchAtLogin;
          });
          step = "pair";
        }}>{t("onboarding.next")}</Button
      >
    {/if}
  {/snippet}
</Dialog>

<style>
  input {
    height: 38px;
    padding: 0 12px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-background);
    user-select: text;
  }
  .launch {
    display: flex;
    align-items: center;
    gap: var(--space-md);
  }
  .grow {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .label {
    font-weight: 500;
  }
  .get-app {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding-top: var(--space-sm);
    border-top: 1px solid var(--color-border);
  }
  .get-app :global(.qr) {
    margin: 0;
    padding: 4px;
  }
</style>
