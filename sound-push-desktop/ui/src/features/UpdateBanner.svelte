<script lang="ts">
  // Non-intrusive update notice at the top of the page: never a dialog, never installs by itself.
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { LINKS, releasePage } from "../lib/links";
  import { run } from "../lib/stores/toast.svelte";
  import { updater } from "../lib/stores/updater.svelte";

  const visible = $derived(
    !updater.dismissed && (updater.status === "available" || updater.status === "downloading" || updater.status === "ready"),
  );
  const percent = $derived(updater.percent);
  const version = $derived(updater.version ?? "");
  /** Announced once per state; progress is exposed by the progress bar instead. */
  const summary = $derived(
    updater.status === "ready"
      ? t("update.ready", version)
      : updater.status === "downloading"
        ? t("update.downloadingUnknown")
        : t("update.available", version),
  );
</script>

{#if visible}
  <section class="update" aria-label={t("settings.updates")}>
    <span class="icon"><Icon name="info" size={18} /></span>
    <div class="text">
      <p aria-hidden="true">
        {updater.status === "downloading" && percent !== null ? t("update.downloading", `${percent}%`) : summary}
      </p>
      <span class="sr-only" role="status">{summary}</span>
      {#if updater.status === "downloading"}
        <div
          class="progress"
          role="progressbar"
          aria-label={t("update.progress")}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent ?? undefined}
        >
          <div class="fill" class:indeterminate={percent === null} style:width={percent === null ? "30%" : `${percent}%`}></div>
        </div>
      {/if}
      {#if updater.errorKey}<p class="caption" role="alert">{t(updater.errorKey)}</p>{/if}
    </div>
    <div class="actions">
      {#if updater.errorKey}
        <Button variant="ghost" onclick={() => run(engine.openUrl(LINKS.releases))}>{t("update.openReleases")}</Button>
      {/if}
      {#if updater.status === "available"}
        <Button variant="ghost" onclick={() => run(engine.openUrl(releasePage(version)))}>{t("update.whatsNew")}</Button>
        <Button variant="primary" onclick={() => updater.download()}>{t("update.download")}</Button>
      {:else if updater.status === "ready"}
        <Button variant="primary" onclick={() => updater.restart()}>{t("update.restart")}</Button>
      {/if}
      {#if updater.status !== "downloading"}
        <Button variant="ghost" icon="close" label={t("update.later")} onclick={() => updater.dismiss()} />
      {/if}
    </div>
  </section>
{/if}

<style>
  .update {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm) var(--space-md);
    max-width: 760px;
    margin-bottom: var(--space-lg);
    padding-block: 10px;
    padding-inline: 14px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    background: var(--color-surface);
  }
  .icon {
    display: grid;
    color: var(--color-accent);
  }
  .text {
    flex: 1;
    min-width: 200px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-xs);
  }
  .progress {
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-muted);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--color-accent);
    transition: width var(--motion-normal);
  }
  .fill.indeterminate {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  @media (forced-colors: active) {
    .progress {
      border: 1px solid CanvasText;
    }
    .fill {
      background: Highlight;
    }
  }
</style>
