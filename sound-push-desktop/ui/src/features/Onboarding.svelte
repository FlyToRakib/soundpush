<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import QrCode from "../lib/components/QrCode.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run } from "../lib/stores/toast.svelte";

  let step = $state<"welcome" | "name" | "pair">("welcome");
  let name = $state(store.state?.settings.deviceName ?? "");
  const startTrusted = store.trustedPeers.length;

  function finish() {
    void engine.stopPairing();
    updateSettings((s) => {
      if (name.trim()) s.deviceName = name.trim();
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
  {:else}
    <p class="muted">{t("pair.scan")}</p>
    {#if store.state?.pairing.qrUri}<QrCode value={store.state.pairing.qrUri} label={t("pair.title")} />{/if}
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
          updateSettings((s) => (s.deviceName = name.trim()));
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
</style>
