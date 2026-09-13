import en from "./en.json";

type Dict = Record<string, string>;
const dictionaries: Record<string, Dict> = { en };
let current: Dict = en;

export function setLanguage(tag: string): void {
  const lang = tag === "system" ? navigator.language : tag;
  current = dictionaries[lang.split("-")[0] ?? "en"] ?? en;
}

/** Translate `key`, replacing {0}, {1}… with args. Falls back to English, then the key. */
export function t(key: string, ...args: (string | number)[]): string {
  const text = current[key] ?? (en as Dict)[key] ?? key;
  return args.reduce<string>((s, a, i) => s.replaceAll(`{${i}}`, String(a)), text);
}

export function formatElapsed(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const mm = String(m).padStart(h > 0 ? 2 : 1, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
