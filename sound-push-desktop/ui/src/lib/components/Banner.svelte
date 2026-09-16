<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import Button from "./Button.svelte";

  let {
    severity = "info",
    message,
    code,
    actionLabel,
    onaction,
    ondismiss,
    live = true,
  }: {
    severity?: "info" | "warning" | "error";
    message: string;
    /** Stable support code (e.g. "SP-NET-004"), shown after the message so it can be quoted. */
    code?: string;
    actionLabel?: string;
    onaction?: () => void;
    ondismiss?: () => void;
    /** Announce the message itself. Turn off inside a container that is already a live region. */
    live?: boolean;
  } = $props();
</script>

<div class="banner {severity}" role={live ? (severity === "error" ? "alert" : "status") : undefined}>
  <Icon name={severity === "info" ? "info" : "alert"} size={18} />
  <p>{message}{#if code}<span class="support-code">{code}</span>{/if}</p>
  {#if actionLabel && onaction}<Button variant="secondary" onclick={onaction}>{actionLabel}</Button>{/if}
  {#if ondismiss}<Button variant="ghost" icon="close" label={t("common.dismiss")} onclick={ondismiss} />{/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding-block: 8px;
    padding-inline: 12px 8px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  p {
    flex: 1;
  }
  /* Support code: readable and selectable, but never louder than the message itself. */
  .support-code {
    margin-inline-start: var(--space-xs);
    color: var(--color-text-secondary);
    font-family: ui-monospace, "Cascadia Mono", Menlo, monospace;
    font-size: 0.85em;
    user-select: text;
  }
  .warning :global(svg) {
    color: var(--color-warning);
  }
  .error :global(svg:first-child) {
    color: var(--color-danger);
  }
  .info :global(svg:first-child) {
    color: var(--color-accent);
  }
</style>
