<script lang="ts">
  import Button from "./Button.svelte";
  import { formatAccelerator, recordKey } from "../hotkeys";
  import { t } from "../i18n";

  let {
    value,
    label,
    mac = false,
    onchange,
  }: { value: string | null; label: string; mac?: boolean; onchange: (accelerator: string | null) => void } = $props();

  let recording = $state(false);
  const display = $derived(recording ? t("hotkey.recording") : value ? formatAccelerator(value, mac) : t("audio.hotkeyNone"));

  function onkeydown(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const result = recordKey(e);
    if (result.kind === "pending") return;
    recording = false;
    if (result.kind === "clear") onchange(null);
    else if (result.kind === "set") onchange(result.accelerator);
  }
</script>

<div class="hotkey">
  <button
    class="field"
    class:recording
    aria-label={`${label}: ${display}`}
    aria-pressed={recording}
    onclick={() => (recording = !recording)}
    {onkeydown}
    onblur={() => (recording = false)}
  >
    <span aria-live="polite">{display}</span>
  </button>
  {#if value && !recording}<Button variant="ghost" icon="close" label={t("hotkey.clear")} onclick={() => onchange(null)} />{/if}
</div>

<style>
  .hotkey {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .field {
    min-width: 160px;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    font-family: ui-monospace, "Cascadia Mono", Menlo, monospace;
    cursor: pointer;
  }
  .field.recording {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }
</style>
