import { engine } from "../engine/client";
import type { Settings } from "../engine/types";
import { store } from "./engine.svelte";
import { run } from "./toast.svelte";

/** Apply a change to a copy of the current settings and save it through the engine. */
export function updateSettings(mutate: (s: Settings) => void): void {
  const current = store.state?.settings;
  if (!current) return;
  // Reactive proxies can't be structured-cloned; a JSON copy is exact for this plain data.
  const next = JSON.parse(JSON.stringify(current)) as Settings;
  mutate(next);
  void run(engine.updateSettings(next));
}
