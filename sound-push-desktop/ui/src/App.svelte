<script lang="ts">
  import { tick } from "svelte";
  import Button from "./lib/components/Button.svelte";
  import Icon, { type IconName } from "./lib/components/Icon.svelte";
  import { engine } from "./lib/engine/client";
  import { store } from "./lib/stores/engine.svelte";
  import { updater } from "./lib/stores/updater.svelte";
  import { platform } from "./lib/stores/platform.svelte";
  import { ui, type Page } from "./lib/stores/ui.svelte";
  import { setLanguage, t } from "./lib/i18n";
  import Home from "./features/Home.svelte";
  import Devices from "./features/Devices.svelte";
  import Audio from "./features/Audio.svelte";
  import Settings from "./features/Settings.svelte";
  import Overlays from "./features/Overlays.svelte";
  import Onboarding from "./features/Onboarding.svelte";
  import UpdateBanner from "./features/UpdateBanner.svelte";

  const nav: { id: Page; icon: IconName }[] = [
    { id: "home", icon: "home" },
    { id: "devices", icon: "devices" },
    { id: "audio", icon: "audio" },
    { id: "settings", icon: "settings" },
  ];
  let main = $state<HTMLElement>();

  const app = $derived(store.state);
  // Resolving the language also sets `lang` and `dir` on the document. The keyed block below
  // renders every text again when it changes.
  const language = $derived(setLanguage(app?.settings.language ?? "system"));
  const onboarding = $derived(app !== null && !app.settings.dismissedTips.includes("onboarding"));
  const modifier = $derived(app?.local.platform === "macos" ? "Meta" : "Control");

  const autoUpdate = $derived(app?.settings.checkForUpdates ?? false);
  $effect(() => {
    updater.setAutomatic(autoUpdate);
    return () => updater.setAutomatic(false);
  });

  // Firewall, permissions and media checks start once the engine is up.
  $effect(() => {
    if (app) platform.start();
  });

  /** Ctrl+1…4 (⌘1…4 on macOS) switch pages; focus moves to the new page for screen readers. */
  function onkeydown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || !app) return;
    const item = nav[Number(e.key) - 1];
    if (!item || document.querySelector("dialog[open]")) return;
    e.preventDefault();
    ui.page = item.id;
    void tick().then(() => main?.focus());
  }
</script>

<svelte:window {onkeydown} />

{#key language}
  {#if app}
    <div class="shell">
      <nav aria-label={t("nav.label")}>
        <div class="brand">
          <span class="logo" aria-hidden="true"><Icon name="audio" size={18} /></span>
          {t("app.name")}
        </div>
        {#each nav as item, i (item.id)}
          <button
            type="button"
            class="nav-item"
            aria-current={ui.page === item.id ? "page" : undefined}
            aria-keyshortcuts={`${modifier}+${i + 1}`}
            onclick={() => (ui.page = item.id)}
          >
            <Icon name={item.icon} />
            {t(`nav.${item.id}`)}
          </button>
        {/each}
        <div class="spacer"></div>
        <div class="self caption" title={app.local.displayCode}>
          <span class="dot" aria-hidden="true"></span>{app.local.name}
        </div>
      </nav>

      <main bind:this={main} tabindex="-1" aria-label={t(`nav.${ui.page}`)}>
        <UpdateBanner />
        {#if ui.page === "home"}
          <Home onnavigate={(p) => (ui.page = p)} />
        {:else if ui.page === "devices"}
          <Devices />
        {:else if ui.page === "audio"}
          <Audio />
        {:else}
          <Settings />
        {/if}
      </main>
    </div>

    <Overlays />
    {#if onboarding}<Onboarding />{/if}
  {:else if store.startError}
    <div class="starting" role="alert">
      <span class="failed-icon" aria-hidden="true"><Icon name="alert" size={26} /></span>
      <p class="failed-title">{t("app.failed")}</p>
      <p class="caption failed-reason">{store.startError}</p>
      <p class="caption">{t("app.failedHint")}</p>
      <Button variant="primary" onclick={() => void engine.openLogsFolder()}>{t("app.openLogs")}</Button>
    </div>
  {:else}
    <div class="starting" role="status">
      <span class="spinner" aria-hidden="true"></span>
      <p>{t("app.starting")}</p>
      <p class="caption">{t("app.startingHint")}</p>
    </div>
  {/if}
{/key}

<style>
  .shell {
    display: grid;
    grid-template-columns: 212px 1fr;
    height: 100%;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-md) var(--space-sm);
    background: var(--color-surface);
    border-inline-end: 1px solid var(--color-border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    font-weight: 600;
    padding: 4px 10px 16px;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: var(--color-accent);
    color: var(--color-on-accent);
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    text-align: start;
  }
  .nav-item:hover {
    background: var(--color-surface-muted);
  }
  .nav-item[aria-current="page"] {
    background: var(--color-surface-muted);
    color: var(--color-text-primary);
    font-weight: 500;
  }
  @media (forced-colors: active) {
    .nav-item[aria-current="page"] {
      outline: 2px solid Highlight;
    }
  }
  .self {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-success);
    flex-shrink: 0;
  }
  main {
    min-width: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: var(--space-lg) var(--space-xl);
  }
  /* Focus lands on the page after a shortcut; the page itself is not a control, so no ring. */
  main:focus {
    outline: none;
  }
  .starting {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-sm);
    text-align: center;
  }
  .failed-icon {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 12px;
    background: var(--color-surface-muted);
    color: var(--color-accent);
    margin-bottom: var(--space-sm);
  }
  .failed-title {
    font-weight: 600;
  }
  .failed-reason {
    max-width: 420px;
    word-break: break-word;
  }
  .spinner {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 3px solid var(--color-border);
    border-top-color: var(--color-accent);
    animation: spin 0.9s linear infinite;
    margin-bottom: var(--space-sm);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
    }
  }
</style>
