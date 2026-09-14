<script lang="ts">
  // One-time hint when the window is closed but SoundPush keeps running (plan §24 "Close button").
  import { onMount } from "svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import { engine, onCloseHint } from "../lib/engine/client";
  import type { Settings } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { run } from "../lib/stores/toast.svelte";

  /** null: hidden; true: the tray icon is visible; false: this desktop has no tray. */
  let hint = $state<boolean | null>(null);
  const isMac = $derived(store.state?.local.platform === "macos");

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void onCloseHint((tray) => (hint = tray)).then((u) => (gone ? u() : (unlisten = u)));
    return () => {
      gone = true;
      unlisten?.();
    };
  });

  /** Remember the hint, then close. Saving first: closing destroys this page. */
  async function keepRunning() {
    hint = null;
    const current = store.state?.settings;
    if (current && !current.dismissedTips.includes("closeToTray")) {
      const next = JSON.parse(JSON.stringify(current)) as Settings;
      next.dismissedTips.push("closeToTray");
      await run(engine.updateSettings(next));
    }
    await run(engine.closeMainWindow());
  }
</script>

{#if hint !== null}
  <Dialog title={t("closeHint.title")} onclose={() => (hint = null)}>
    <p class="muted">
      {#if !hint}{t("closeHint.noTray")}{:else if isMac}{t("closeHint.bodyMac")}{:else}{t("closeHint.body")}{/if}
    </p>
    {#snippet actions()}
      <Button onclick={() => run(engine.quitApp())}>{t("closeHint.quit")}</Button>
      <Button variant="primary" onclick={keepRunning}>{hint ? t("closeHint.ok") : t("closeHint.minimize")}</Button>
    {/snippet}
  </Dialog>
{/if}
