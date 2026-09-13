<script lang="ts" generics="T extends string">
  let {
    value,
    options,
    label,
    onchange,
  }: { value: T; options: { value: T; label: string }[]; label: string; onchange: (v: T) => void } = $props();

  function onkeydown(e: KeyboardEvent) {
    const i = options.findIndex((o) => o.value === value);
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = options[(i + step + options.length) % options.length];
    if (next) onchange(next.value);
  }
</script>

<div class="segmented" role="radiogroup" aria-label={label} tabindex="-1" {onkeydown}>
  {#each options as option (option.value)}
    <button
      role="radio"
      aria-checked={option.value === value}
      tabindex={option.value === value ? 0 : -1}
      onclick={() => onchange(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-control);
    background: var(--color-surface-muted);
    border: 1px solid var(--color-border);
  }
  button {
    border: 0;
    background: transparent;
    padding: 5px 12px;
    border-radius: 6px;
    cursor: pointer;
    color: var(--color-text-secondary);
    transition: background var(--motion-fast), color var(--motion-fast);
  }
  button[aria-checked="true"] {
    background: var(--color-surface);
    color: var(--color-text-primary);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.12);
  }
</style>
