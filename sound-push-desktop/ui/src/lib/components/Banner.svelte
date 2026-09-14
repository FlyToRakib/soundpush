<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import Button from "./Button.svelte";

  let {
    severity = "info",
    message,
    actionLabel,
    onaction,
    ondismiss,
    live = true,
  }: {
    severity?: "info" | "warning" | "error";
    message: string;
    actionLabel?: string;
    onaction?: () => void;
    ondismiss?: () => void;
    /** Announce the message itself. Turn off inside a container that is already a live region. */
    live?: boolean;
  } = $props();
</script>

<div class="banner {severity}" role={live ? (severity === "error" ? "alert" : "status") : undefined}>
  <Icon name={severity === "info" ? "info" : "alert"} size={18} />
  <p>{message}</p>
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
