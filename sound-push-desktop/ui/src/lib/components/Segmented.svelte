<script lang="ts" generics="T extends string">
  let {
    value,
    options,
    label,
    onchange,
  }: { value: T; options: { value: T; label: string }[]; label: string; onchange: (v: T) => void } = $props();

  let group: HTMLDivElement;

  // Arrow keys move the selection like a native radio group (mirrored in right-to-left layouts);
  // Home/End jump to the ends. Focus follows the selection.
  function onkeydown(e: KeyboardEvent) {
    const i = options.findIndex((o) => o.value === value);
    const rtl = getComputedStyle(group).direction === "rtl";
    let next: number | null = null;
    if (e.key === "ArrowDown" || e.key === (rtl ? "ArrowLeft" : "ArrowRight")) next = i + 1;
    else if (e.key === "ArrowUp" || e.key === (rtl ? "ArrowRight" : "ArrowLeft")) next = i - 1;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = options.length - 1;
    if (next === null) return;
    e.preventDefault();
    const index = (next + options.length) % options.length;
    const option = options[index];
    if (!option) return;
    onchange(option.value);
    group.querySelectorAll<HTMLButtonElement>("button")[index]?.focus();
  }
</script>

<div bind:this={group} class="segmented" role="radiogroup" aria-label={label} tabindex="-1" {onkeydown}>
  {#each options as option (option.value)}
    <button
      type="button"
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
    flex-wrap: wrap;
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
  /* Windows high-contrast themes drop backgrounds; keep the selection visible. */
  @media (forced-colors: active) {
    button[aria-checked="true"] {
      outline: 2px solid Highlight;
    }
  }
</style>
