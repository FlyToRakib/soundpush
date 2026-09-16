<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import { engine } from "../lib/engine/client";
  import { t } from "../lib/i18n";
  import { store } from "../lib/stores/engine.svelte";
  import { platform } from "../lib/stores/platform.svelte";
  import { updateSettings } from "../lib/stores/settings";
  import { run } from "../lib/stores/toast.svelte";
  import { ui } from "../lib/stores/ui.svelte";
  import { TOPICS, diagnose, type StepAction, type Topic } from "../lib/troubleshoot";

  let open = $state<Topic | null>(null);
  let checking = $state(false);
  let virtualMicInstalled = $state<boolean | null>(null);

  const steps = $derived(
    open && store.state
      ? diagnose(open, { state: store.state, network: platform.network, system: platform.system, virtualMicInstalled })
      : [],
  );

  async function check() {
    checking = true;
    try {
      await platform.refresh();
      virtualMicInstalled = (await engine.virtualMicStatus()).installed;
    } catch {
      virtualMicInstalled = null;
    } finally {
      checking = false;
    }
  }

  function toggle(topic: Topic) {
    open = open === topic ? null : topic;
    if (open) void check();
  }

  // Opened from elsewhere (a notice's fix button).
  $effect(() => {
    if (ui.troubleshoot) {
      open = ui.troubleshoot;
      ui.troubleshoot = null;
      void check();
    }
  });

  function act(action: StepAction) {
    switch (action) {
      case "pair":
        ui.page = "devices";
        break;
      case "fixFirewall":
        ui.firewallFix = true;
        break;
      case "openAudio":
        ui.page = "audio";
        break;
      case "openNetworkSettings":
        void run(engine.openSystemSettings("network"));
        break;
      case "openMicSettings":
        void run(engine.openSystemSettings("microphone"));
        break;
      case "openSystemAudioSettings":
        void run(engine.openSystemSettings("systemAudio"));
        break;
      case "openSoundSettings":
        void run(engine.openSystemSettings("sound"));
        break;
      case "optionalFeatures":
        void run(engine.openSystemSettings("optionalFeatures"));
        break;
      case "setStable":
        updateSettings((x) => (x.stream.latency = "stable"));
        break;
      case "preventSleep":
        updateSettings((x) => (x.desktop.preventSleepWhileStreaming = true));
        break;
      case "unmuteMic":
        void run(engine.setMicMuted(false));
        break;
      case "useAutoVirtualMic":
        updateSettings((x) => (x.desktop.virtualMicDevice = null));
        break;
      case "visibilityTrusted":
        updateSettings((x) => (x.visibility = "trustedOnly"));
        break;
      case "openUsb":
        // Devices is where USB over adb is set up, the way past a network that isolates clients.
        ui.page = "devices";
        break;
    }
  }
</script>

<div class="topics">
  {#each TOPICS as topic (topic)}
    <button class="trouble" aria-expanded={open === topic} onclick={() => toggle(topic)}>{t(`trouble.${topic}`)}</button>
    {#if open === topic}
      <ol class="steps" aria-busy={checking} aria-label={t(`trouble.${topic}`)}>
        {#each steps as step (step.key + (step.args ?? []).join())}
          <li class="step {step.status}">
            <span class="mark" aria-hidden="true"></span>
            <span class="text">
              <span class="sr-only">{t(`trouble.status.${step.status}`)}:</span>
              {t(step.key, ...(step.args ?? []))}
            </span>
            {#if step.action}
              {@const action = step.action}
              <Button onclick={() => act(action)}>{t(`trouble.action.${action}`)}</Button>
            {/if}
          </li>
        {/each}
      </ol>
      <div class="again">
        <Button variant="ghost" disabled={checking} onclick={check}
          >{t(checking ? "trouble.checking" : "trouble.checkAgain")}</Button
        >
      </div>
    {/if}
  {/each}
</div>

<style>
  .topics {
    display: flex;
    flex-direction: column;
  }
  .trouble {
    text-align: left;
    padding: 8px 0;
    border: 0;
    border-bottom: 1px solid var(--color-border);
    background: transparent;
    cursor: pointer;
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 8px 0 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .text {
    flex: 1;
  }
  .mark {
    flex-shrink: 0;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--color-success);
  }
  .problem .mark {
    background: var(--color-danger);
  }
  .tip .mark {
    background: var(--color-warning);
  }
  .again {
    padding: 4px 0 12px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
