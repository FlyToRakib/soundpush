<script lang="ts">
  import Icon, { type IconName } from "./lib/components/Icon.svelte";
  import { store } from "./lib/stores/engine.svelte";
  import { setLanguage, t } from "./lib/i18n";
  import Home from "./features/Home.svelte";
  import Devices from "./features/Devices.svelte";
  import Audio from "./features/Audio.svelte";
  import Settings from "./features/Settings.svelte";
  import Overlays from "./features/Overlays.svelte";
  import Onboarding from "./features/Onboarding.svelte";

  type Page = "home" | "devices" | "audio" | "settings";
  const nav: { id: Page; icon: IconName }[] = [
    { id: "home", icon: "home" },
    { id: "devices", icon: "devices" },
    { id: "audio", icon: "audio" },
    { id: "settings", icon: "settings" },
  ];
  let page = $state<Page>("home");

  const app = $derived(store.state);
  const onboarding = $derived(app !== null && !app.settings.dismissedTips.includes("onboarding"));
  $effect(() => {
    if (app) setLanguage(app.settings.language);
  });
</script>

{#if app}
  <div class="shell">
    <nav aria-label="Main">
      <div class="brand">
        <span class="logo" aria-hidden="true"><Icon name="audio" size={18} /></span>
        {t("app.name")}
      </div>
      {#each nav as item (item.id)}
        <button class="nav-item" aria-current={page === item.id ? "page" : undefined} onclick={() => (page = item.id)}>
          <Icon name={item.icon} />
          {t(`nav.${item.id}`)}
        </button>
      {/each}
      <div class="spacer"></div>
      <div class="self caption" title={app.local.displayCode}>
        <span class="dot" aria-hidden="true"></span>{app.local.name}
      </div>
    </nav>

    <main>
      {#if page === "home"}
        <Home onnavigate={(p) => (page = p)} />
      {:else if page === "devices"}
        <Devices />
      {:else if page === "audio"}
        <Audio />
      {:else}
        <Settings />
      {/if}
    </main>
  </div>

  <Overlays />
  {#if onboarding}<Onboarding />{/if}
{:else}
  <div class="starting" role="status">
    <span class="spinner" aria-hidden="true"></span>
    <p>{t("app.starting")}</p>
    <p class="caption">{t("app.startingHint")}</p>
  </div>
{/if}

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
    border-right: 1px solid var(--color-border);
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
    height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    text-align: left;
  }
  .nav-item:hover {
    background: var(--color-surface-muted);
  }
  .nav-item[aria-current="page"] {
    background: var(--color-surface-muted);
    color: var(--color-text-primary);
    font-weight: 500;
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
  .starting {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-sm);
    text-align: center;
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
