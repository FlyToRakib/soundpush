<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import QualityBadge from "../lib/components/QualityBadge.svelte";
  import { engine } from "../lib/engine/client";
  import type { PeerView } from "../lib/engine/types";
  import { formatList, t, tp } from "../lib/i18n";
  import { TASKS, type Task, platformIcon } from "../lib/routes";
  import { store } from "../lib/stores/engine.svelte";
  import { run, toasts } from "../lib/stores/toast.svelte";
  import PairDialog from "./PairDialog.svelte";
  import PlatformBanners from "./PlatformBanners.svelte";
  import RouteCard from "./RouteCard.svelte";

  let { onnavigate }: { onnavigate: (page: "devices" | "audio") => void } = $props();

  const app = $derived(store.state!);
  const active = $derived(app.routes);
  let pairing = $state(false);
  let picking = $state<Task | null>(null);
  // Follows live state: devices that disconnect disappear, and the dialog closes when none are left.
  const pickable = $derived(picking ? store.connectedPeers : []);
  const capableIds = $derived(new Set(picking ? peersForTask(picking).map((p) => p.deviceId) : []));

  function peersForTask(task: Task): PeerView[] {
    return task.kinds
      .map((k) => store.peersFor(k))
      .reduce((acc, list) => acc.filter((p) => list.some((q) => q.deviceId === p.deviceId)));
  }

  /** A start that needs "Replace current microphone source?" before it can go ahead (plan §8.2). */
  let takeover = $state<{ task: Task; peer: PeerView } | null>(null);

  async function startTask(task: Task, peer: PeerView, replace = false) {
    picking = null;
    for (const kind of task.kinds) {
      try {
        await engine.startRoute(peer.deviceId, kind, replace);
      } catch (e) {
        // Another device is the peer's microphone: ask before taking it over.
        if ((e as { key?: string }).key === "error.audio.virtualMicBusy" && !replace) {
          takeover = { task, peer };
          return;
        }
        await run(Promise.reject(e));
      }
    }
  }

  /** A task runs with a peer when every one of its route kinds is open with that peer. */
  function runs(task: Task, peerId: string): boolean {
    return task.kinds.every((k) => active.some((r) => r.peerId === peerId && r.kind === k && r.status !== "stopped"));
  }

  /** Peers the task runs with. A larger task (headset) claims its parts, so it lights one card, not three. */
  function activePeersFor(task: Task): PeerView[] {
    return store.trustedPeers.filter(
      (p) =>
        runs(task, p.deviceId) &&
        !TASKS.some(
          (o) =>
            o !== task &&
            o.kinds.length > task.kinds.length &&
            task.kinds.every((k) => o.kinds.includes(k)) &&
            runs(o, p.deviceId),
        ),
    );
  }

  const names = (peers: PeerView[]) => formatList(peers.map((p) => p.name));

  function onTask(task: Task) {
    if (!task.available(app)) {
      onnavigate("audio");
      return;
    }
    const connected = store.connectedPeers;
    const peers = peersForTask(task);
    const running = activePeersFor(task);
    // Already running with the only device there is: nothing to start, point at Stop.
    if (running.length > 0 && connected.length <= 1) toasts.show(t("home.alreadyActive", names(running)));
    else if (connected.length === 0) toasts.show(t("home.noConnected"), "warning");
    else if (peers.length === 0) toasts.show(t("home.noCapable"), "warning");
    // With one connected device there is nothing to choose; with several, always ask.
    else if (connected.length === 1 && peers[0]) void startTask(task, peers[0]);
    else picking = task;
  }

  $effect(() => {
    if (picking && pickable.length === 0) picking = null;
  });
</script>

<div class="page stack">
  <PlatformBanners />
  {#if store.trustedPeers.length === 0}
    <section class="empty">
      <span class="empty-icon"><Icon name="phone" size={28} /></span>
      <h1>{t("home.empty.title")}</h1>
      <p class="muted">{t("home.empty.body")}</p>
      <Button variant="primary" icon="qr" onclick={() => (pairing = true)}>{t("home.pair")}</Button>
    </section>
  {:else}
    {#if active.length > 0}
      <section class="stack" aria-labelledby="active-title">
        <h2 id="active-title">{t("home.active")}</h2>
        {#each active as route (route.routeId)}
          <RouteCard {route} />
        {/each}
      </section>
    {/if}

    <section class="stack" aria-labelledby="tasks-title">
      <h1 id="tasks-title">{t("home.title")}</h1>
      <div class="tasks">
        {#each TASKS as task (task.id)}
          {@const available = task.available(app)}
          {@const running = activePeersFor(task)}
          <button
            class="task"
            class:unavailable={!available}
            class:active={running.length > 0}
            aria-pressed={running.length > 0}
            onclick={() => onTask(task)}
          >
            <span class="task-icon"><Icon name={task.icon} size={22} /></span>
            <span class="task-text">
              <strong>{t(`task.${task.id}`)}</strong>
              <span class="caption">
                {#if running.length > 0}
                  {t("task.active", names(running))}
                {:else if !available}
                  {t(task.unavailableKey ?? "")}
                {:else if task.id === "receiveMicToVirtualMic" && app.capabilities.virtualMicInput}
                  {t("task.receiveMicToVirtualMic.descNamed", app.capabilities.virtualMicInput)}
                {:else}
                  {t(`task.${task.id}.desc`)}
                {/if}
              </span>
            </span>
            {#if running.length > 0}<span class="task-badge">{t("task.activeBadge")}</span>{/if}
          </button>
        {/each}
      </div>
    </section>

    <section class="stack" aria-labelledby="devices-title">
      <div class="row">
        <h2 id="devices-title">{t("devices.paired")}</h2>
        <div class="spacer"></div>
        <Button variant="ghost" icon="plus" onclick={() => (pairing = true)}>{t("devices.pair")}</Button>
      </div>
      <ul class="peers" aria-label={tp("devices.pairedCount", store.trustedPeers.length)}>
        {#each store.trustedPeers as peer (peer.deviceId)}
          <li>
            <button class="peer" onclick={() => onnavigate("devices")}>
              <Icon name={platformIcon(peer.platform)} />
              <span class="peer-name">{peer.name}</span>
              <span class="caption">{t(`status.${peer.connection}`)}</span>
              <QualityBadge quality={peer.quality} />
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

{#if pairing}<PairDialog onclose={() => (pairing = false)} />{/if}

{#if picking && pickable.length > 0}
  <Dialog title={t("task.pickDevice")} onclose={() => (picking = null)}>
    {#each pickable as peer (peer.deviceId)}
      {@const alreadyRunning = picking ? runs(picking, peer.deviceId) : false}
      {@const supported = capableIds.has(peer.deviceId) && !alreadyRunning}
      <button class="peer" disabled={!supported} onclick={() => picking && startTask(picking, peer)}>
        <Icon name={platformIcon(peer.platform)} />
        <span class="peer-name">
          {peer.name}
          {#if !supported}<span class="caption">{t(alreadyRunning ? "task.peerActive" : "task.peerUnsupported")}</span>{/if}
        </span>
        {#if supported}<Icon name="chevron" size={16} />{/if}
      </button>
    {/each}
  </Dialog>
{/if}

{#if takeover}
  {@const pending = takeover}
  <Dialog title={t("mic.takeover.title")} onclose={() => (takeover = null)}>
    <p class="muted">{t("mic.takeover.body", pending.peer.name)}</p>
    {#snippet actions()}
      <Button onclick={() => (takeover = null)}>{t("common.cancel")}</Button>
      <Button
        variant="primary"
        onclick={() => {
          takeover = null;
          void startTask(pending.task, pending.peer, true);
        }}>{t("mic.takeover.replace")}</Button
      >
    {/snippet}
  </Dialog>
{/if}

<style>
  .page {
    max-width: 760px;
    gap: var(--space-xl);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: var(--space-sm);
    padding: 72px var(--space-lg);
  }
  .empty p {
    max-width: 360px;
    margin-bottom: var(--space-md);
  }
  .empty-icon,
  .task-icon {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 12px;
    background: var(--color-surface-muted);
    color: var(--color-accent);
  }
  .tasks {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: var(--space-sm);
  }
  .task {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-md);
    border-radius: var(--radius-card);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    text-align: start;
    cursor: pointer;
    transition: border-color var(--motion-fast);
  }
  .task:hover {
    border-color: var(--color-accent);
  }
  .task.unavailable .task-icon {
    color: var(--color-text-secondary);
  }
  /* The running task: accent outline and tint, solid icon tile, "Active" badge. */
  .task.active {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, var(--color-surface));
  }
  .task.active .task-icon {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }
  .task.active .caption {
    color: var(--color-accent);
  }
  .task-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .task-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: 999px;
    background: var(--color-accent);
    color: var(--color-on-accent);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }
  .task-badge::before {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }
  .peers {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .peer {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: 10px 12px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    cursor: pointer;
    text-align: start;
  }
  .peer:hover:not(:disabled) {
    background: var(--color-surface-muted);
  }
  .peer:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .peer-name {
    flex: 1;
    display: flex;
    flex-direction: column;
    font-weight: 500;
  }
</style>
