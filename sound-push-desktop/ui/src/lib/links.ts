// Public pages the app links to. They open in the default browser (never inside the webview).
const REPO = "https://github.com/FlyToRakib/soundpush";

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

/** Release notes page for a version ("0.2.0"). */
export function releasePage(version: string): string {
  return `${LINKS.releases}/tag/v${version}`;
}
