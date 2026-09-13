<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    variant = "secondary",
    icon,
    disabled = false,
    label,
    onclick,
    children,
  }: {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    icon?: IconName;
    disabled?: boolean;
    /** Accessible label for icon-only buttons. */
    label?: string;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  } = $props();
</script>

<button class="btn {variant}" class:icon-only={!children} {disabled} aria-label={label} title={label} {onclick}>
  {#if icon}<Icon name={icon} size={18} />{/if}
  {#if children}<span>{@render children()}</span>{/if}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-sm);
    min-height: 36px;
    padding: 0 14px;
    border-radius: var(--radius-control);
    border: 1px solid transparent;
    background: transparent;
    cursor: pointer;
    font-weight: 500;
    transition: background var(--motion-fast), border-color var(--motion-fast), opacity var(--motion-fast);
    white-space: nowrap;
  }
  .icon-only {
    padding: 0;
    width: 36px;
  }
  .primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }
  .primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .secondary {
    background: var(--color-surface);
    border-color: var(--color-border);
  }
  .secondary:hover:not(:disabled),
  .ghost:hover:not(:disabled) {
    background: var(--color-surface-muted);
  }
  .danger {
    color: var(--color-danger);
    border-color: var(--color-border);
    background: var(--color-surface);
  }
  .danger:hover:not(:disabled) {
    background: var(--color-surface-muted);
  }
  .btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
