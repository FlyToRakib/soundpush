<script lang="ts">
  // One UAC prompt has to leave the phone able to connect, so the choices here are the ones that
  // actually decide that: the rule alone is useless while Windows calls the network public.
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { platform } from "../lib/stores/platform.svelte";
  import { run, toasts } from "../lib/stores/toast.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const net = $derived(platform.network);
  const network = $derived(net?.publicNetworkName ?? t("firewall.thisNetwork"));
  /** On a public network the rule needs one of the two choices below to have any effect. */
  const onPublic = $derived(Boolean(net?.publicNetwork));

  // Both stay off until the user picks: one loosens the firewall on untrusted networks, the
  // other changes how Windows treats the network for every app.
  let includePublic = $state(false);
  let makePrivate = $state(false);
  let working = $state(false);
  /** What the last attempt left behind, shown here rather than as a toast over the dialog. */
  let outcome = $state<string | null>(null);

  const resolvesPublic = $derived(!onPublic || includePublic || (makePrivate && Boolean(net?.canMakePrivate)));

  async function allow() {
    if (working || !resolvesPublic) return;
    working = true;
    outcome = null;
    try {
      const status = await engine.fixFirewall(includePublic, makePrivate && Boolean(net?.canMakePrivate));
      platform.network = status;
      if (!status.blocked) {
        toasts.show(t("firewall.fixed"));
        onclose();
        return;
      }
      // The rule went in but something outside SoundPush still blocks the phone.
      outcome = t(status.publicNetwork ? "firewall.stillPublic" : "firewall.stillBlocked");
    } catch (e) {
      const message = (e as { message?: string }).message ?? String(e);
      // Declining the Windows prompt is a choice, not an error.
      if (!message.includes("cancelled")) outcome = t("firewall.failed", message);
    } finally {
      working = false;
    }
  }
</script>

<Dialog title={t("firewall.title")} {onclose}>
  <p class="muted">{t("firewall.body")}</p>

  {#if onPublic}
    <p>{t("firewall.publicNetwork", network)}</p>
    <div class="choices">
      {#if net?.canMakePrivate}
        <label class="check">
          <input type="checkbox" bind:checked={makePrivate} />
          <span>
            {t("firewall.makePrivate", network)}
            <span class="caption">{t("firewall.makePrivateNote")}</span>
          </span>
        </label>
      {/if}
      <label class="check">
        <input type="checkbox" bind:checked={includePublic} />
        <span>
          {t("firewall.includePublic")}
          <span class="caption">{t("firewall.includePublicNote")}</span>
        </span>
      </label>
    </div>
    {#if !net?.canMakePrivate}
      <p class="caption">
        {t("firewall.cannotMakePrivate")}
        <button class="link" onclick={() => run(engine.openSystemSettings("network"))}>{t("firewall.openNetworkSettings")}</button
        >
      </p>
    {/if}
    {#if !resolvesPublic}
      <p class="caption">{t("firewall.pickOne", network)}</p>
    {/if}
  {:else}
    <label class="check">
      <input type="checkbox" bind:checked={includePublic} />
      <span>{t("firewall.includePublicAhead")}</span>
    </label>
  {/if}

  <p class="caption">{t("firewall.adminNote")}</p>
  {#if outcome}<Banner severity="warning" message={outcome} />{/if}

  {#snippet actions()}
    <Button onclick={onclose}>{t("common.cancel")}</Button>
    <Button variant="primary" disabled={working || !resolvesPublic} onclick={allow}>
      {t(working ? "firewall.working" : "firewall.allow")}
    </Button>
  {/snippet}
</Dialog>

<style>
  p {
    margin: 0 0 var(--space-sm);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    margin-bottom: var(--space-sm);
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--space-sm);
    margin: var(--space-sm) 0;
    cursor: pointer;
  }
  .choices .check {
    margin: 0;
  }
  .check input {
    margin-top: 3px;
  }
  .check .caption {
    display: block;
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
