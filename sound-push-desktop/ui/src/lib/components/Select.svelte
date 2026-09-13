<script lang="ts">
  let {
    value,
    options,
    label,
    id,
    onchange,
  }: {
    value: string;
    options: { value: string; label: string }[];
    label: string;
    id?: string;
    onchange: (v: string) => void;
  } = $props();
</script>

<select {id} aria-label={label} {value} onchange={(e) => onchange((e.currentTarget as HTMLSelectElement).value)}>
  {#each options as option (option.value)}
    <option value={option.value}>{option.label}</option>
  {/each}
</select>

<style>
  select {
    min-width: 200px;
    max-width: 280px;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    cursor: pointer;
  }
</style>
