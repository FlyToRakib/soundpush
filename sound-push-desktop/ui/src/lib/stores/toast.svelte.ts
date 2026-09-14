import { t } from "../i18n";

export interface Toast {
  id: number;
  message: string;
  severity: "info" | "warning" | "error";
}

class ToastStore {
  items = $state<Toast[]>([]);
  /** True while the pointer or keyboard focus is on a message, so nothing closes while it is being read. */
  paused = false;
  private next = 1;

  show(message: string, severity: Toast["severity"] = "info") {
    const id = this.next++;
    this.items = [...this.items, { id, message, severity }];
    this.after(severity === "error" ? 8000 : 4000, () => this.dismiss(id));
  }

  dismiss(id: number) {
    this.items = this.items.filter((i) => i.id !== id);
  }

  /** Run `fn` after `ms`, waiting longer while a message is hovered or focused. */
  after(ms: number, fn: () => void) {
    const tick = () => {
      if (this.paused) setTimeout(tick, 1000);
      else fn();
    };
    setTimeout(tick, ms);
  }
}

export const toasts = new ToastStore();

/** Run an engine command and show a readable error if it fails. */
export async function run<T>(promise: Promise<T>): Promise<T | undefined> {
  try {
    return await promise;
  } catch (e) {
    const err = e as { key?: string; message?: string } | string;
    const message = typeof err === "string" ? err : err.key ? t(err.key, "") : (err.message ?? t("error.internal"));
    toasts.show(message, "error");
    return undefined;
  }
}
