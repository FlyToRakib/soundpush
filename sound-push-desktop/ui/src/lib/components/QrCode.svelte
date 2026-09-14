<script lang="ts">
  import QRCode from "qrcode";

  let { value, label, size = 220 }: { value: string; label: string; size?: number } = $props();
  let svg = $state("");

  $effect(() => {
    // Always dark modules on white, in both themes, for reliable scanning.
    QRCode.toString(value, { type: "svg", margin: 4, errorCorrectionLevel: "M", color: { dark: "#000000", light: "#ffffff" } })
      .then((s) => (svg = s))
      .catch(() => (svg = ""));
  });
</script>

<div class="qr" role="img" aria-label={label} style:width="{size}px" style:height="{size}px">
  {@html svg}
</div>

<style>
  .qr {
    flex-shrink: 0;
    margin: 0 auto;
    padding: 8px;
    background: #ffffff;
    border-radius: var(--radius-card);
  }
  .qr :global(svg) {
    width: 100%;
    height: 100%;
    display: block;
  }
</style>
