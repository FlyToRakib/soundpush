<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import Button from "./Button.svelte";

  let {
    title,
    onclose,
    children,
    actions,
  }: { title: string; onclose?: () => void; children: Snippet; actions?: Snippet } = $props();

  const titleId = $props.id();
  let dialog: HTMLDialogElement;
  $effect(() => {
    // showModal() makes the rest of the window inert, so focus stays inside the dialog.
    // Focus goes back to whatever opened it when the dialog closes.
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    return () => {
      dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  });
</script>

<!-- Escape fires "cancel": it closes dialogs that can be closed and is ignored by ones that need an answer. -->
<dialog bind:this={dialog} aria-labelledby={titleId} oncancel={(e) => { e.preventDefault(); onclose?.(); }}>
  <header>
    <h1 id={titleId}>{title}</h1>
    {#if onclose}<Button variant="ghost" icon="close" label={t("common.close")} onclick={onclose} />{/if}
  </header>
  <div class="body">{@render children()}</div>
  {#if actions}<footer>{@render actions()}</footer>{/if}
</dialog>

<style>
  dialog {
    width: min(440px, calc(100vw - 48px));
    padding: var(--space-lg);
    border: 1px solid var(--color-border);
    border-radius: 16px;
    background: var(--color-surface);
    color: var(--color-text-primary);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.25);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.35);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-md);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-sm);
    margin-top: var(--space-lg);
  }
</style>
