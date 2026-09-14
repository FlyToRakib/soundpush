// Thin wrapper over the Tauri updater and process plugins. Update manifests are signed with the
// project's minisign key and verified by the plugin before anything is installed.
// Outside Tauri (browser preview, tests) there is never an update.

export interface AvailableUpdate {
  version: string;
  notes: string;
  /** Downloads the signed package; `total` is unknown when the server sends no length. */
  download(onProgress: (downloaded: number, total: number | null) => void): Promise<void>;
  /** Installs the downloaded package. On Windows this closes SoundPush and runs the installer. */
  install(): Promise<void>;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (!inTauri) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check({ timeout: 30_000 });
  if (!update) return null;
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
