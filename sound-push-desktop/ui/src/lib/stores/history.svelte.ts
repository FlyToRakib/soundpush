// Latency and loss of each active route over the last minutes, for the chart in Connection
// details. Kept outside the route card so collapsing it loses nothing; bounded per route.
import { appendSample, type Sample } from "../connection";
import type { RouteView } from "../engine/types";

class HistoryStore {
  series = $state.raw<Record<string, readonly Sample[]>>({});

  /** Called with every engine state; routes that ended are dropped. */
  record(routes: readonly RouteView[], now = Date.now()): void {
    const next: Record<string, readonly Sample[]> = {};
    let changed = Object.keys(this.series).length !== routes.length;
    for (const route of routes) {
      const previous = this.series[route.routeId] ?? [];
      const updated =
        route.status === "active"
          ? appendSample(previous, { at: now, latencyMs: route.stats.latencyMs, lossPct: route.stats.lossPct })
          : previous;
      if (updated !== previous || !(route.routeId in this.series)) changed = true;
      next[route.routeId] = updated;
    }
    if (changed) this.series = next;
  }
}

export const history = new HistoryStore();
