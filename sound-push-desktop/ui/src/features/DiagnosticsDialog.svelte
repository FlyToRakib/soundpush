<script lang="ts">
  // Shows what a diagnostics export contains and what is removed before anything is written
  // (plan §28.2 "User previews contents").
  import { onMount } from "svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import { engine } from "../lib/engine/client";
  import type { DiagnosticsPreview } from "../lib/engine/types";
  import { formatList, t } from "../lib/i18n";
  import { run, toasts } from "../lib/stores/toast.svelte";

  let { onclose }: { onclose: () => void } = $props();
  let preview = $state<DiagnosticsPreview | null>(null);
  let failed = $state(false);
  let saving = $state(false);

  onMount(() => {
    engine
      .previewDiagnostics()
      .then((p) => (preview = p))
      .catch(() => (failed = true));
  });

  async function save() {
    saving = true;
    const path = await run(engine.exportDiagnostics());
    saving = false;
    if (path) {
      toasts.show(t("settings.exported", path));
      onclose();
    }
  }
</script>

<Dialog title={t("diag.title")} {onclose}>
  <p class="muted">{t("diag.body")}</p>
  {#if preview}
    <ul class="sections">
      {#each preview.sections as section (section.id)}
        <li>
          <span>{t(`diag.section.${section.id}`, section.count)}</span>
          {#if section.redacted.length > 0}
            <span class="caption">{formatList(section.redacted.map((r) => t(`diag.redacted.${r}`)))}</span>
          {/if}
        </li>
      {/each}
    </ul>
    <details>
      <summary>{t("diag.showAll")}</summary>
      <!-- Read-only text area: focusable and scrollable with the keyboard, and selectable. -->
      <textarea readonly rows="10" aria-label={t("diag.contents")} value={preview.text}></textarea>
    </details>
  {:else if failed}
    <p role="alert">{t("diag.failed")}</p>
  {:else}
    <p role="status">{t("diag.loading")}</p>
  {/if}

  {#snippet actions()}
    <Button onclick={onclose}>{t("common.cancel")}</Button>
    <Button variant="primary" disabled={!preview || saving} onclick={save}>{t("diag.save")}</Button>
  {/snippet}
</Dialog>

<style>
  .sections {
    margin: 0;
    padding-inline-start: 20px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .sections li span {
    display: block;
  }
  summary {
    cursor: pointer;
  }
  textarea {
    box-sizing: border-box;
    width: 100%;
    height: 220px;
    resize: none;
    overflow: auto;
    margin: 8px 0 0;
    color: var(--color-text-primary);
    padding: 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-background);
    font-family: ui-monospace, "Cascadia Mono", Menlo, monospace;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
</style>
