<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { platform } from "../lib/stores/platform.svelte";
  import { run, toasts } from "../lib/stores/toast.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const net = $derived(platform.network);
  // Public networks (cafés, hotels) stay closed unless the user explicitly opens them.
  let includePublic = $state(false);
  let working = $state(false);

  async function allow() {
    if (working) return;
    working = true;
    try {
      const status = await engine.fixFirewall(includePublic);
      platform.network = status;
      if (status.blocked) {
        toasts.show(t(status.publicNetwork && !includePublic ? "firewall.stillPublic" : "firewall.stillBlocked"), "warning");
      } else {
        toasts.show(t("firewall.fixed"));
        onclose();
      }
    } catch (e) {
      const message = (e as { message?: string }).message ?? String(e);
      // Declining the Windows prompt is a choice, not an error.
      if (!message.includes("cancelled")) toasts.show(t("firewall.failed", message), "error");
    } finally {
      working = false;
    }
  }
</script>

<Dialog title={t("firewall.title")} {onclose}>
  <p class="muted">{t("firewall.body")}</p>
  {#if net?.publicNetwork}
    <p class="caption">
      {t("firewall.publicNetwork", net.publicNetworkName ?? t("firewall.thisNetwork"))}
      <button class="link" onclick={() => run(engine.openSystemSettings("network"))}>{t("firewall.makePrivate")}</button>
    </p>
  {/if}
  <label class="check">
    <input type="checkbox" bind:checked={includePublic} />
    <span>{t("firewall.includePublic")}</span>
  </label>
  <p class="caption">{t("firewall.adminNote")}</p>
  {#snippet actions()}
    <Button onclick={onclose}>{t("common.cancel")}</Button>
    <Button variant="primary" disabled={working} onclick={allow}>{t(working ? "firewall.working" : "firewall.allow")}</Button>
  {/snippet}
</Dialog>

<style>
  p {
    margin: 0 0 var(--space-sm);
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--space-sm);
    margin: var(--space-sm) 0;
    cursor: pointer;
  }
  .check input {
    margin-top: 3px;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--color-accent);
    cursor: pointer;
    font: inherit;
  }
</style>
