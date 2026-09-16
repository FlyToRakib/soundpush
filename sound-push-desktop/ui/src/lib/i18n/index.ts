// Lightweight i18n: flat i18next-style JSON dictionaries, one file per language (`<tag>.json`).
// English (`en.json`) is the source and the fallback for every missing string.
// See docs/translating.md for the workflow.
import en from "./en.json";

export type Dict = Record<string, string>;

/** Pseudo-locales for layout testing: long accented text, and right-to-left. Never shipped as files. */
export const PSEUDO_LONG = "en-XA";
export const PSEUDO_RTL = "ar-XB";

const RTL_LANGUAGES = new Set(["ar", "fa", "he", "iw", "ps", "sd", "ug", "ur", "yi", "dv", "ckb"]);

/** Every `<tag>.json` next to this file is a language; the file name is its BCP-47 tag. */
const files = import.meta.glob<Dict>("./*.json", { eager: true, import: "default" });
export const dictionaries: Record<string, Dict> = { en };
for (const [path, dict] of Object.entries(files)) {
  const tag = path.replace(/^\.\//, "").replace(/\.json$/, "");
  dictionaries[tag] = dict;
}

let locale = "en";
let current: Dict = en;
const pseudoCache = new Map<string, Dict>();

/** Language tags that can be chosen in Settings, English first. Pseudo-locales only in development. */
export function availableLanguages(includePseudo = import.meta.env.DEV): string[] {
  const tags = Object.keys(dictionaries).sort((a, b) => (a === "en" ? -1 : b === "en" ? 1 : a.localeCompare(b)));
  return includePseudo ? [...tags, PSEUDO_LONG, PSEUDO_RTL] : tags;
}

/** The name of a language in that language ("Deutsch"), for the language picker. */
export function languageName(tag: string): string {
  if (tag === PSEUDO_LONG) return "Pseudo-locale (long text)";
  if (tag === PSEUDO_RTL) return "Pseudo-locale (right to left)";
  try {
    const name = new Intl.DisplayNames([tag], { type: "language" }).of(tag);
    if (name) return name.charAt(0).toLocaleUpperCase(tag) + name.slice(1);
  } catch {
    // Unknown tag: show it as is.
  }
  return tag;
}

/** Pick the best available dictionary for a tag: exact match, then the base language, then English. */
export function resolveLanguage(tag: string, preferred: readonly string[] = systemLanguages()): string {
  const candidates = tag === "system" ? preferred : [tag];
  for (const candidate of candidates) {
    if (candidate === PSEUDO_LONG || candidate === PSEUDO_RTL) return candidate;
    if (dictionaries[candidate]) return candidate;
    const base = candidate.split("-")[0] ?? "";
    if (dictionaries[base]) return base;
  }
  return "en";
}

function systemLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return ["en"];
  return navigator.languages?.length ? navigator.languages : [navigator.language];
}

export function isRtl(tag: string): boolean {
  return tag === PSEUDO_RTL || RTL_LANGUAGES.has((tag.split("-")[0] ?? "").toLowerCase());
}

/** Switch the UI language ("system" follows the OS). Also sets `lang` and `dir` on the document. */
export function setLanguage(tag: string): string {
  locale = resolveLanguage(tag);
  current = dictionaryFor(locale);
  if (typeof document !== "undefined") {
    document.documentElement.lang = locale;
    document.documentElement.dir = isRtl(locale) ? "rtl" : "ltr";
  }
  return locale;
}

export function currentLanguage(): string {
  return locale;
}

function dictionaryFor(tag: string): Dict {
  if (tag !== PSEUDO_LONG && tag !== PSEUDO_RTL) return dictionaries[tag] ?? en;
  let dict = pseudoCache.get(tag);
  if (!dict) {
    dict = Object.fromEntries(Object.entries(en as Dict).map(([k, v]) => [k, pseudoLocalize(v, tag === PSEUDO_RTL)]));
    pseudoCache.set(tag, dict);
  }
  return dict;
}

/** Translate `key`, replacing {0}, {1}… with args. Falls back to English, then the key. */
export function t(key: string, ...args: (string | number)[]): string {
  const text = current[key] ?? (en as Dict)[key] ?? key;
  return interpolate(text, args);
}

/**
 * Plural-aware translation (i18next suffixes): looks up `key_one`, `key_few`, `key_other`… for `count`
 * using the language's plural rules. `{0}` is the formatted count; further args are {1}, {2}…
 */
export function tp(key: string, count: number, ...args: (string | number)[]): string {
  const fromCurrent = pluralText(current, intlLocale(), key, count);
  const text = fromCurrent ?? pluralText(en, "en", key, count) ?? key;
  return interpolate(text, [formatNumber(count), ...args]);
}

function pluralText(dict: Dict, lang: string, key: string, count: number): string | undefined {
  const category = new Intl.PluralRules(lang).select(count);
  return dict[`${key}_${category}`] ?? dict[`${key}_other`];
}

function interpolate(text: string, args: (string | number)[]): string {
  return args.reduce<string>((s, a, i) => s.replaceAll(`{${i}}`, String(a)), text);
}

/** Locale used for Intl formatting (pseudo-locales format like their base language). */
function intlLocale(): string {
  return locale === PSEUDO_LONG ? "en" : locale === PSEUDO_RTL ? "ar" : locale;
}

export function formatNumber(value: number, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(intlLocale(), options).format(value);
}

export function formatDateTime(date: Date | number, options?: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(intlLocale(), options ?? { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/** "Pixel, Laptop and Tablet" in the current language. */
export function formatList(items: string[]): string {
  return new Intl.ListFormat(intlLocale(), { style: "long", type: "conjunction" }).format(items);
}

export function formatElapsed(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const mm = String(m).padStart(h > 0 ? 2 : 1, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

const ACCENTS: Record<string, string> = {
  a: "á",
  b: "ƀ",
  c: "ç",
  d: "ð",
  e: "é",
  f: "ƒ",
  g: "ĝ",
  h: "ĥ",
  i: "í",
  j: "ĵ",
  k: "ķ",
  l: "ļ",
  m: "ɱ",
  n: "ñ",
  o: "ó",
  p: "þ",
  q: "ǫ",
  r: "ŕ",
  s: "š",
  t: "ţ",
  u: "ú",
  v: "ṽ",
  w: "ŵ",
  x: "ẋ",
  y: "ý",
  z: "ž",
  A: "Á",
  B: "Ɓ",
  C: "Ç",
  D: "Ð",
  E: "É",
  F: "Ƒ",
  G: "Ĝ",
  H: "Ĥ",
  I: "Í",
  J: "Ĵ",
  K: "Ķ",
  L: "Ļ",
  M: "Ṁ",
  N: "Ñ",
  O: "Ó",
  P: "Þ",
  Q: "Ǫ",
  R: "Ŕ",
  S: "Š",
  T: "Ţ",
  U: "Ú",
  V: "Ṽ",
  W: "Ŵ",
  X: "Ẋ",
  Y: "Ý",
  Z: "Ž",
};

/**
 * Pseudo-translate a string: accented letters, about 40 % longer, wrapped in ⟦ ⟧ so clipped or
 * untranslated (hard-coded) text is easy to spot. Placeholders like {0} are kept intact.
 * With `rtl`, the text is wrapped in right-to-left marks so mirrored layouts can be checked.
 */
export function pseudoLocalize(text: string, rtl = false): string {
  const parts = text.split(/(\{\d+\})/);
  let letters = 0;
  const body = parts
    .map((part) => {
      if (/^\{\d+\}$/.test(part)) return part;
      letters += part.replace(/\s/g, "").length;
      return rtl ? part : [...part].map((ch) => ACCENTS[ch] ?? ch).join("");
    })
    .join("");
  const padding = "·".repeat(Math.ceil(letters * 0.4));
  const expanded = padding ? `${body} ${padding}` : body;
  return rtl ? `‫⟦${expanded}⟧‬` : `⟦${expanded}⟧`;
}
