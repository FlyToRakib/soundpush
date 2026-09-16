// Public pages the app links to. They open in the default browser (never inside the webview).
const REPO = "https://github.com/FlyToRakib/soundpush";
/** Documentation site built from docs/ (mkdocs.yml, .github/workflows/docs.yml). */
export const DOCS_SITE = "https://flytorakib.github.io/soundpush";

export const LINKS = {
  source: REPO,
  userGuide: `${REPO}/blob/HEAD/docs/user-guide.md`,
  privacy: `${REPO}/blob/HEAD/PRIVACY.md`,
  license: `${REPO}/blob/HEAD/LICENSE`,
  reportBug: `${REPO}/issues/new/choose`,
  releases: `${REPO}/releases`,
  /** Where the Android app is downloaded (onboarding QR code). */
  androidApp: `${REPO}/releases/latest`,
  translate: `${REPO}/blob/HEAD/docs/translating.md`,
} as const;

/** Help pages: the page on the documentation site, and the same text on GitHub. */
const DOCS = {
  userGuide: { site: "user-guide/", github: LINKS.userGuide },
  troubleshooting: { site: "troubleshooting/", github: `${LINKS.userGuide}#9-troubleshooting` },
  virtualMicrophone: { site: "virtual-microphone/", github: `${REPO}/blob/HEAD/docs/virtual-microphone.md` },
  privacy: { site: "privacy/", github: LINKS.privacy },
  translating: { site: "translating/", github: `${REPO}/blob/HEAD/docs/translating.md` },
  /** The hidden troubleshooting overrides in Settings → Help → Advanced. */
  advanced: { site: "advanced/", github: `${REPO}/blob/HEAD/docs/advanced.md` },
} as const;

export type DocsPage = keyof typeof DOCS;

/**
 * URL of a help page: the documentation site when it answers, otherwise the page on GitHub
 * (site not published yet, GitHub Pages down, or a network that blocks it).
 */
export async function docsUrl(
  page: DocsPage,
  fetcher: typeof fetch = (input, init) => fetch(input, init),
  timeoutMs = 3000,
): Promise<string> {
  const { site, github } = DOCS[page];
  const url = `${DOCS_SITE}/${site}`;
  try {
    const response = await fetcher(url, { method: "HEAD", cache: "no-store", signal: AbortSignal.timeout(timeoutMs) });
    return response.ok ? url : github;
  } catch {
    return github;
  }
}

/** Release notes page for a version ("0.2.0"). */
export function releasePage(version: string): string {
  return `${LINKS.releases}/tag/v${version}`;
}
