<script lang="ts">
  let {
    value,
    min,
    max,
    step = 1,
    label,
    id,
    format = (v: number) => String(v),
    onchange,
  }: {
    value: number;
    min: number;
    max: number;
    step?: number;
    label: string;
    id?: string;
    format?: (v: number) => string;
    onchange: (v: number) => void;
  } = $props();
</script>

<div class="slider">
  <input
    {id}
    type="range"
    {min}
    {max}
    {step}
    {value}
    aria-label={label}
    aria-valuetext={format(value)}
    oninput={(e) => onchange(Number((e.currentTarget as HTMLInputElement).value))}
  />
  <span class="value caption">{format(value)}</span>
</div>

<style>
  .slider {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  input {
    width: 180px;
    accent-color: var(--color-accent);
  }
  .value {
    min-width: 52px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
