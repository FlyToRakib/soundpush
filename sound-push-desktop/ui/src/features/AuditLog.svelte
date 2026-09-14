<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import { auditText } from "../lib/audit";
  import { engine } from "../lib/engine/client";
  import type { AuditEntry } from "../lib/engine/types";
  import { formatDateTime, t } from "../lib/i18n";
  import { run } from "../lib/stores/toast.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let entries = $state<AuditEntry[] | null>(null);
  let confirming = $state(false);

  async function load() {
    entries = (await run(engine.auditLog())) ?? [];
  }

  async function clear() {
    confirming = false;
    await run(engine.clearAuditLog());
    await load();
  }

  $effect(() => {
    void load();
  });
</script>

<Dialog title={t("audit.title")} {onclose}>
  <p class="caption">{t("audit.desc")}</p>
  {#if entries === null}
    <p class="muted" role="status">{t("audit.loading")}</p>
  {:else if entries.length === 0}
    <p class="muted">{t("audit.empty")}</p>
  {:else}
    <ul class="entries">
      {#each entries as entry, i (i)}
        <li>
          <span>{auditText(entry, t)}</span>
          <span class="caption">{formatDateTime(entry.timeUnix * 1000, { dateStyle: "medium", timeStyle: "short" })}</span>
        </li>
      {/each}
    </ul>
  {/if}
  {#if confirming}
    <p class="muted" role="alert">{t("audit.clear.confirm")}</p>
  {/if}
  {#snippet actions()}
    {#if confirming}
      <Button onclick={() => (confirming = false)}>{t("common.cancel")}</Button>
      <Button variant="danger" onclick={clear}>{t("audit.clear")}</Button>
    {:else}
      <Button disabled={!entries?.length} onclick={() => (confirming = true)}>{t("audit.clear")}</Button>
      <Button variant="primary" onclick={onclose}>{t("common.close")}</Button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 50vh;
    overflow-y: auto;
    user-select: text;
  }
  li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-xs) 0;
    border-bottom: 1px solid var(--color-border);
  }
  li:last-child {
    border-bottom: none;
  }
</style>
