// Thin wrapper over the Tauri updater and process plugins. Update manifests are signed with the
// project's minisign key and verified by the plugin before anything is installed.
// The desktop side (`check_update` in src-tauri/src/commands.rs) picks the manifest for the update channel;
// staged rollout is applied here (./rollout.ts).
// Outside Tauri (browser preview, tests) there is never an update.
import { offerUpdate } from "./rollout";
import type { UpdateChannel } from "./types";

export interface AvailableUpdate {
  version: string;
  notes: string;
  /** Downloads the signed package; `total` is unknown when the server sends no length. */
  download(onProgress: (downloaded: number, total: number | null) => void): Promise<void>;
  /** Installs the downloaded package. On Windows this closes SoundPush and runs the installer. */
  install(): Promise<void>;
}

export interface CheckOptions {
  channel: UpdateChannel;
  /** Stable per-install id used for the staged-rollout bucket (the device id). */
  installId: string;
  /** The user asked; staged rollout does not hold the update back (a halted one still does). */
  manual: boolean;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function checkForUpdate({ channel, installId, manual }: CheckOptions): Promise<AvailableUpdate | null> {
  if (!inTauri) return null;
  const [{ invoke }, { Update }] = await Promise.all([import("@tauri-apps/api/core"), import("@tauri-apps/plugin-updater")]);
  const metadata = await invoke<ConstructorParameters<typeof Update>[0] | null>("check_update", {
    channel,
    timeoutMs: 30_000,
  });
  if (!metadata) return null;
  const update = new Update(metadata);
  if (!offerUpdate(update.rawJson, installId, update.version, manual)) {
    void update.close();
    return null;
  }
  return {
    version: update.version,
    notes: update.body ?? "",
    async download(onProgress) {
      let downloaded = 0;
      let total: number | null = null;
      await update.download((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? null;
        else if (event.event === "Progress") downloaded += event.data.chunkLength;
        onProgress(downloaded, total);
      });
    },
    install: () => update.install(),
  };
}

export async function relaunch(): Promise<void> {
  if (!inTauri) return;
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
