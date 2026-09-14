# Translating SoundPush

SoundPush ships in English today. The apps are ready for more languages: every string is externalised, plural
forms and number formats follow the language, and layouts work right to left. Community translation through
Weblate is planned (docs/soundpush-final.md §23.6, Phase 3); until then, translations arrive as pull requests.

## Where the strings live

| App | Source (English) | Translations |
|---|---|---|
| Desktop | `sound-push-desktop/ui/src/lib/i18n/en.json` | `sound-push-desktop/ui/src/lib/i18n/<tag>.json`, e.g. `de.json`, `pt-BR.json` |
| Android | `sound-push-mobile/android/core-ui/src/main/res/values/strings.xml` | `…/res/values-<lang>/strings.xml`, e.g. `values-de`, `values-pt-rBR` |

Use the same concept for the same key on both apps where possible (for example "Paired devices only").

## Desktop format

Flat [i18next](https://www.i18next.com/misc/json-format)-style JSON: one key per string.

```json
{
  "task.active": "Streaming to {0}",
  "devices.pairedCount_one": "{0} paired device",
  "devices.pairedCount_other": "{0} paired devices"
}
```

- **Placeholders** `{0}`, `{1}` are filled in by the app (a device name, a number). Keep every placeholder; move it
  wherever the grammar needs it.
- **Plurals** use i18next suffixes: `_zero`, `_one`, `_two`, `_few`, `_many`, `_other`. Provide the forms your language
  needs according to the [CLDR plural rules](https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html).
  `_other` is always required. In plural strings `{0}` is the count, already formatted for the language.
- Missing keys fall back to English, so partial translations are fine.
- Do not translate product names: **SoundPush**, **VB-CABLE**, **VB-Audio**, **BlackHole**, **PipeWire**.
- Keep the tone of the English copy (docs/soundpush-final.md §23.6): plain verbs, short sentences, no technical
  terms like "server" or "player", and errors that say what happened and what to do.

The app picks up every `<tag>.json` file automatically; no code change is needed. The language list in
Settings → General shows each language by its own name.

## Android format

Standard Android resources. Placeholders are `%1$s`, `%2$d`; plurals use `<plurals>` with `quantity` items.
Escape apostrophes as `\'`. Android mirrors layouts for right-to-left languages automatically
(`android:supportsRtl="true"`).

The language list in Settings → General → Language (and Android 13+'s per-app language screen in system settings)
is generated from the `values-<lang>` folders at build time (`generateLocaleConfig` in `app/build.gradle.kts`), so a
new folder is all it takes. Android stores the choice as the app's per-app language; the app mirrors it to the
engine's `language` setting. Numbers and units (`%1$d ms`, `%1$d kb/s`) are strings too, so they can be reordered.

## Adding a language

1. Copy `en.json` to `<tag>.json` (BCP-47 tag: `de`, `es`, `pt-BR`, `zh-Hans`) and translate the values.
2. For Android, create `values-<lang>/strings.xml` with the translated strings (only the ones you translated). Use
   Android's resource qualifiers: `values-de`, `values-pt-rBR`, `values-b+zh+Hans`. Build with
   `./gradlew assembleDebug testDebugUnitTest` (in `sound-push-mobile/android`); the screenshot tests render Home and
   Settings right to left, and the images land in `app/build/outputs/roborazzi/`.
3. Check your work:
   ```bash
   npm --prefix sound-push-desktop/ui test   # fails on unknown keys or changed placeholders
   npm --prefix sound-push-desktop/ui run dev # then Settings → General → Language
   ```
4. Look at every page in Light and Dark. Long words must not be cut off; buttons may wrap.
5. Open a pull request titled `i18n: add <language>`. Say whether a native speaker reviewed it.

## Testing layouts without a translation (pseudo-locales)

Two pseudo-locales are generated from English at runtime (they are never shipped as files):

- **`en-XA` — long text:** accented letters, about 40 % longer, wrapped in `⟦ ⟧`. Text that stays plain English is
  hard-coded and must be moved into `en.json`; text whose `⟧` is missing is cut off.
- **`ar-XB` — right to left:** English text laid out right to left, so mirrored layouts (sidebar, toggles, chevrons,
  toasts) can be checked.

In a development build (`npm run dev` or `tauri dev`) both appear under Settings → General → Language. In a release
build, set `"language": "en-XA"` (or `"ar-XB"`) in `settings.json` in the SoundPush data folder while the app is
closed; the language list then shows it too.

On Android, debug builds generate the same two pseudo-locales from `strings.xml` (`isPseudoLocalesEnabled`) and list
them under Settings → General → Language.

## Right-to-left languages

The desktop sets `dir="rtl"` for Arabic, Hebrew, Persian, Urdu and other RTL languages. When writing CSS, use logical
properties (`margin-inline-start`, `inset-inline-end`, `text-align: start`) instead of left/right, and give icons
that point along the reading direction `data-icon="chevron"`-style mirroring (see `app.css`).

## Formatting numbers, dates and lists

Use the helpers in `lib/i18n` instead of building strings by hand: `formatNumber`, `formatDateTime`, `formatList`
("Pixel, Laptop and Tablet") and `tp` for plurals. They follow the chosen language.
