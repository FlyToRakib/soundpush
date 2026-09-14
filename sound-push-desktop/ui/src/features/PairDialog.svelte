<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import QrCode from "../lib/components/QrCode.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { run } from "../lib/stores/toast.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let manual = $state(false);
  let address = $state("");
  let now = $state(Date.now() / 1000);

  const pairing = $derived(store.state?.pairing);
  const remaining = $derived(Math.max(0, Math.round((pairing?.qrExpiresUnix ?? 0) - now)));
  const startTrusted = store.trustedPeers.length;

  $effect(() => {
    void run(engine.startPairing());
    const timer = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => {
      clearInterval(timer);
      void engine.stopPairing();
    };
  });

  // Close automatically once a new device is paired.
  $effect(() => {
    if (store.trustedPeers.length > startTrusted) onclose();
  });

  // Regenerate the code when it expires while the dialog is open: one attempt per few seconds,
  // so a failing engine is not asked again on every tick.
  let lastRenew = 0;
  $effect(() => {
    if (pairing && !pairing.qrUri && remaining === 0 && now - lastRenew > 5) {
      lastRenew = now;
      void run(engine.startPairing());
    }
  });

  async function connectManual(e: SubmitEvent) {
    e.preventDefault();
    if (address.trim()) await run(engine.pairWithAddress(address.trim()));
  }
</script>

<Dialog title={t("pair.title")} {onclose}>
  {#if !manual}
    <p class="muted center">{t("pair.scan")}</p>
    {#if pairing?.qrUri}
      <QrCode value={pairing.qrUri} label={t("pair.title")} />
      <p class="caption center">
        {t("pair.expires", `${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`)}
      </p>
    {/if}
    <Button variant="ghost" onclick={() => (manual = true)}>{t("pair.manual")}</Button>
  {:else}
    <form class="stack" onsubmit={connectManual}>
      <label for="pair-address">{t("pair.address")}</label>
      <input id="pair-address" bind:value={address} placeholder="192.168.1.20" autocomplete="off" spellcheck="false" />
      <p class="caption">{t("pair.addressHint")}</p>
      <div class="row">
        <Button variant="ghost" onclick={() => (manual = false)}>{t("common.back")}</Button>
        <div class="spacer"></div>
        <Button type="submit" variant="primary" disabled={!address.trim()}>{t("pair.connect")}</Button>
      </div>
    </form>
  {/if}
</Dialog>

<style>
  .center {
    text-align: center;
  }
  input {
    height: 36px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-background);
    user-select: text;
  }
  label {
    font-weight: 500;
  }
</style>
