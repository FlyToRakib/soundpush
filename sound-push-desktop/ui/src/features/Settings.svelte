<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Card from "../lib/components/Card.svelte";
  import Segmented from "../lib/components/Segmented.svelte";
  import Select from "../lib/components/Select.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { engine } from "../lib/engine/client";
  import type { Theme, Visibility } from "../lib/engine/types";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run, toasts } from "../lib/stores/toast.svelte";
  import { platform } from "../lib/stores/platform.svelte";
  import Troubleshooter from "./Troubleshooter.svelte";

  const app = $derived(store.state!);
  const s = $derived(app.settings);
  const isMac = $derived(app.local.platform === "macos");
  /** Launch at login is on here but switched off in the OS (Task Manager / Startup apps). */
  const startupBlocked = $derived(s.desktop.launchAtLogin && platform.system?.autostartDisabledByOs === true);

  async function exportDiagnostics() {
    const path = await run(engine.exportDiagnostics());
    if (path) toasts.show(t("settings.exported", path));
  }
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
        options={[
          { value: "system", label: t("theme.system") },
          { value: "en", label: "English" },
        ]}
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
      <Toggle checked={s.desktop.launchAtLogin} label={t("settings.launchAtLogin")}
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
    <div class="row">
      <Button onclick={exportDiagnostics}>{t("settings.export")}</Button>
      <Button variant="ghost" onclick={() => run(engine.openLogsFolder())}>{t("settings.openLogs")}</Button>
    </div>
  </Card>

  <Card title={t("settings.about")}>
    <SettingRow label={t("settings.thisDevice")} description={app.local.addresses.slice(0, 2).join(" · ")}>
      <span class="caption code">{app.local.displayCode}</span>
    </SettingRow>
    <p class="caption">{t("settings.version", app.local.appVersion)} · {t("settings.license")}</p>
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
</style>
