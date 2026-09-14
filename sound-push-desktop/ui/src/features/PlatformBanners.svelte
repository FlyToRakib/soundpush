<script lang="ts">
  // Problems with the computer itself that stop SoundPush from working, each with its fix.
  import Banner from "../lib/components/Banner.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { platform } from "../lib/stores/platform.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run } from "../lib/stores/toast.svelte";
  import { ui } from "../lib/stores/ui.svelte";

  const net = $derived(platform.network);
  const sys = $derived(platform.system);
  const dismissed = $derived(store.state?.settings.dismissedTips ?? []);
</script>

{#if net?.firewallEnabled && net.blocked}
  <Banner severity="warning" message={t("banner.firewall")} actionLabel={t("banner.firewall.fix")} onaction={() => (ui.firewallFix = true)} />
{/if}
{#if sys?.systemAudio === "denied"}
  <Banner
    severity="warning"
    message={t("banner.systemAudioDenied")}
    actionLabel={t("common.openSettings")}
    onaction={() => run(engine.openSystemSettings("systemAudio"))}
  />
{/if}
{#if sys?.mediaFeaturePackMissing && !dismissed.includes("mediaFeaturePack")}
  <Banner
    severity="info"
    message={t("banner.mediaFeaturePack", sys.windowsEdition ?? "Windows")}
    actionLabel={t("banner.mediaFeaturePack.fix")}
    onaction={() => run(engine.openSystemSettings("optionalFeatures"))}
    ondismiss={() => updateSettings((x) => x.dismissedTips.push("mediaFeaturePack"))}
  />
{/if}
