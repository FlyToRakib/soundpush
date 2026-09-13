import type { Theme } from "../engine/types";

/** Apply Light / Dark / System. "System" follows the OS live through `prefers-color-scheme`. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  try {
    localStorage.setItem("sp-theme", theme);
  } catch {
    // storage unavailable: theme still applies for this session
  }
}
