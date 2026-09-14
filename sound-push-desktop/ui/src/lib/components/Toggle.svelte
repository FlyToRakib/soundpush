<script lang="ts">
  let {
    checked,
    label,
    id,
    disabled = false,
    onchange,
  }: { checked: boolean; label: string; id?: string; disabled?: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button
  {id}
  type="button"
  class="toggle"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="thumb"></span>
</button>

<style>
  .toggle {
    position: relative;
    flex-shrink: 0;
    width: 40px;
    height: 22px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--color-border);
    background: var(--color-surface-muted);
    cursor: pointer;
    padding: 0;
    transition: background var(--motion-fast);
  }
  .thumb {
    position: absolute;
    top: 2px;
    inset-inline-start: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--color-text-secondary);
    transition: transform var(--motion-fast), background var(--motion-fast);
  }
  .toggle[aria-checked="true"] {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }
  .toggle[aria-checked="true"] .thumb {
    transform: translateX(18px);
    background: var(--color-on-accent);
  }
  :global([dir="rtl"]) .toggle[aria-checked="true"] .thumb {
    transform: translateX(-18px);
  }
  .toggle:disabled {
    opacity: 0.45;
    cursor: default;
  }
  /* Windows high-contrast themes: draw the switch with system colors. */
  @media (forced-colors: active) {
    .toggle {
      border-color: ButtonText;
    }
    .thumb {
      background: ButtonText;
    }
    .toggle[aria-checked="true"] .thumb {
      background: Highlight;
    }
  }
</style>
