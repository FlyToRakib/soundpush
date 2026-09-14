<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Card from "../lib/components/Card.svelte";
  import Segmented from "../lib/components/Segmented.svelte";
  import Select from "../lib/components/Select.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { engine } from "../lib/engine/client";
  import type { Theme, UpdateChannel, Visibility } from "../lib/engine/types";
  import { PSEUDO_LONG, PSEUDO_RTL, availableLanguages, languageName, t } from "../lib/i18n";
  import { LINKS } from "../lib/links";
  import { store } from "../lib/stores/engine.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run, toasts } from "../lib/stores/toast.svelte";
  import { updater } from "../lib/stores/updater.svelte";
  import { platform } from "../lib/stores/platform.svelte";
  import Troubleshooter from "./Troubleshooter.svelte";

  const app = $derived(store.state!);
  const s = $derived(app.settings);
  const isMac = $derived(app.local.platform === "macos");
  /** Launch at login is on here but switched off in the OS (Task Manager / Startup apps). */
  const startupBlocked = $derived(s.desktop.launchAtLogin && platform.system?.autostartDisabledByOs === true);

  // Pseudo-locales appear in development builds, or once chosen (e.g. set by a tester).
  const languages = $derived([
    { value: "system", label: t("theme.system") },
    ...availableLanguages(import.meta.env.DEV || s.language === PSEUDO_LONG || s.language === PSEUDO_RTL).map(
      (tag) => ({ value: tag, label: languageName(tag) }),
    ),
  ]);

  const updateText = $derived.by(() => {
    if (updater.errorKey) return t(updater.errorKey);
    switch (updater.status) {
      case "checking":
        return t("update.checking");
      case "upToDate":
        return t("update.upToDate");
      case "available":
        return t("update.available", updater.version ?? "");
      case "downloading":
        return updater.percent === null ? t("update.downloadingUnknown") : t("update.downloading", `${updater.percent}%`);
      case "ready":
        return t("update.ready", updater.version ?? "");
      default:
        return "";
    }
  });

  async function exportDiagnostics() {
    const path = await run(engine.exportDiagnostics());
    if (path) toasts.show(t("settings.exported", path));
  }

  const open = (url: string) => run(engine.openUrl(url));
</script>

<div class="page stack">
  <h1>{t("settings.title")}</h1>

  <Card title={t("settings.general")}>
    <SettingRow label={t("settings.deviceName")} description={t("settings.deviceName.desc")} id="device-name">
      <input
        id="device-name"
        class="text-input"
        value={s.deviceName}
        maxlength="64"
        onchange={(e) => {
          const v = (e.currentTarget as HTMLInputElement).value.trim();
          if (v) updateSettings((x) => (x.deviceName = v));
        }}
      />
    </SettingRow>
    <SettingRow label={t("settings.theme")}>
      <Segmented
        value={s.theme}
        label={t("settings.theme")}
        options={(["system", "light", "dark"] as Theme[]).map((v) => ({ value: v, label: t(`theme.${v}`) }))}
        onchange={(v) => updateSettings((x) => (x.theme = v))}
      />
    </SettingRow>
    <SettingRow label={t("settings.language")}>
      <Select
        value={s.language}
        label={t("settings.language")}
        options={languages}
        onchange={(v) => updateSettings((x) => (x.language = v))}
      />
    </SettingRow>
    <SettingRow
      label={isMac ? t("settings.launchAtLogin.mac") : t("settings.launchAtLogin")}
      description={startupBlocked ? t("settings.launchAtLogin.disabledByOs") : undefined}
    >
      {#if startupBlocked}
        <Button variant="ghost" onclick={() => run(engine.openSystemSettings("startup"))}>{t("common.openSettings")}</Button>
      {/if}
      <Toggle checked={s.desktop.launchAtLogin} label={isMac ? t("settings.launchAtLogin.mac") : t("settings.launchAtLogin")}
        onchange={(v) => updateSettings((x) => (x.desktop.launchAtLogin = v))} />
    </SettingRow>
    <SettingRow label={t("settings.startMinimized")}>
      <Toggle checked={s.desktop.startMinimized} label={t("settings.startMinimized")}
        onchange={(v) => updateSettings((x) => (x.desktop.startMinimized = v))} />
    </SettingRow>
    <SettingRow label={t("settings.closeToTray")}>
      <Toggle checked={s.desktop.closeToTray} label={t("settings.closeToTray")}
        onchange={(v) => updateSettings((x) => (x.desktop.closeToTray = v))} />
    </SettingRow>
    <SettingRow label={t("settings.preventSleep")}>
      <Toggle checked={s.desktop.preventSleepWhileStreaming} label={t("settings.preventSleep")}
        onchange={(v) => updateSettings((x) => (x.desktop.preventSleepWhileStreaming = v))} />
    </SettingRow>
    <SettingRow label={t("settings.audioCues")} description={t("settings.audioCues.desc")}>
      <Toggle checked={s.audioCues} label={t("settings.audioCues")} onchange={(v) => updateSettings((x) => (x.audioCues = v))} />
    </SettingRow>
  </Card>

  <Card title={t("settings.privacy")} description={t("settings.privacyNote")}>
    <SettingRow label={t("settings.visibility")}>
      <Select
        value={s.visibility}
        label={t("settings.visibility")}
        options={(["everyone", "trustedOnly", "hidden"] as Visibility[]).map((v) => ({ value: v, label: t(`visibility.${v}`) }))}
        onchange={(v) => updateSettings((x) => (x.visibility = v as Visibility))}
      />
    </SettingRow>
    <SettingRow label={t("settings.autoConnect")}>
      <Toggle checked={s.autoConnectTrusted} label={t("settings.autoConnect")}
        onchange={(v) => updateSettings((x) => (x.autoConnectTrusted = v))} />
    </SettingRow>
  </Card>

  <Card title={t("settings.help")}>
    <Troubleshooter />
    <div class="row wrap">
      <Button onclick={exportDiagnostics}>{t("settings.export")}</Button>
      <Button variant="ghost" onclick={() => run(engine.openLogsFolder())}>{t("settings.openLogs")}</Button>
      <Button variant="ghost" onclick={() => open(LINKS.userGuide)}>{t("settings.userGuide")}</Button>
      <Button variant="ghost" onclick={() => open(LINKS.reportBug)}>{t("settings.reportBug")}</Button>
    </div>
    <p class="caption">{t("settings.shortcuts", isMac ? "⌘" : "Ctrl")}</p>
  </Card>

  <Card title={t("settings.about")}>
    <SettingRow label={t("settings.thisDevice")} description={app.local.addresses.slice(0, 2).join(" · ")}>
      <span class="caption code">{app.local.displayCode}</span>
    </SettingRow>
    <p class="caption">{t("settings.version", app.local.appVersion)} · {t("settings.license")}</p>

    <SettingRow label={t("update.auto")} description={t("update.auto.desc")}>
      <Toggle checked={s.checkForUpdates} label={t("update.auto")}
        onchange={(v) => updateSettings((x) => (x.checkForUpdates = v))} />
    </SettingRow>
    <SettingRow label={t("update.channel")} description={t("update.channel.desc")} id="update-channel">
      <Select
        id="update-channel"
        value={s.updateChannel}
        label={t("update.channel")}
        options={[
          { value: "stable", label: t("update.channel.stable") },
          { value: "beta", label: t("update.channel.beta") },
        ]}
        onchange={(v) => updateSettings((x) => (x.updateChannel = v as UpdateChannel))} />
    </SettingRow>
    <div class="row wrap">
      <p class="caption grow" role="status">{updateText}</p>
      {#if updater.status === "available"}
        <Button variant="primary" onclick={() => updater.download()}>{t("update.download")}</Button>
      {:else if updater.status === "ready"}
        <Button variant="primary" onclick={() => updater.restart()}>{t("update.restart")}</Button>
      {:else}
        <Button disabled={updater.busy} onclick={() => updater.check(true)}>{t("update.check")}</Button>
      {/if}
    </div>

    <div class="row wrap">
      <Button variant="ghost" onclick={() => open(LINKS.privacy)}>{t("settings.privacyPolicy")}</Button>
      <Button variant="ghost" onclick={() => open(LINKS.license)}>{t("settings.viewLicense")}</Button>
      <Button variant="ghost" onclick={() => open(LINKS.source)}>{t("settings.source")}</Button>
    </div>
  </Card>
</div>

<style>
  .page {
    max-width: 760px;
  }
  .text-input {
    height: 34px;
    width: 220px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    border: 1px solid var(--color-border);
    background: var(--color-background);
    user-select: text;
  }
  .code {
    font-family: ui-monospace, "Cascadia Mono", Menlo, monospace;
    user-select: text;
  }
  .wrap {
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
    min-width: 200px;
  }
</style>
