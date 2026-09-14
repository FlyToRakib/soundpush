// Desktop UI flows against the mock engine, with an axe check on every page and dialog
// (plan §29.1). Each test loads the page fresh, so the mock starts from the same state.
import AxeBuilder from "@axe-core/playwright";
import { type Page, expect, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

async function expectAccessible(page: Page, what: string) {
  const { violations } = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  const summary = violations.map((v) => `${v.id} (${v.impact}): ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
  expect(summary, `accessibility problems on ${what}`).toEqual([]);
}

/** Open the app and skip onboarding. */
async function open(page: Page) {
  await page.goto("/");
  const welcome = page.getByRole("dialog", { name: "Welcome to SoundPush" });
  await expect(welcome).toBeVisible();
  await welcome.getByRole("button", { name: "Skip" }).click();
  await expect(welcome).toBeHidden();
  await expect(page.getByRole("heading", { name: "What do you want to do?" })).toBeVisible();
}

function nav(page: Page, name: string) {
  return page.getByRole("navigation", { name: "Main" }).getByRole("button", { name });
}

test("onboarding names the computer, asks about sign-in and offers the Android app", async ({ page }) => {
  await page.goto("/");
  const dialog = page.getByRole("dialog");
  await expect(dialog).toHaveAccessibleName("Welcome to SoundPush");
  await expectAccessible(page, "onboarding welcome");
  await dialog.getByRole("button", { name: "Continue" }).click();

  await expect(dialog).toHaveAccessibleName("Name this computer");
  await dialog.getByLabel("Device name").fill("Studio PC");
  const launch = dialog.getByRole("switch", { name: "Start with Windows" });
  await expect(launch).toHaveAttribute("aria-checked", "true");
  await launch.click();
  await expect(launch).toHaveAttribute("aria-checked", "false");
  await expectAccessible(page, "onboarding name step");
  await dialog.getByRole("button", { name: "Continue" }).click();

  await expect(dialog).toHaveAccessibleName("Pair a device");
  await expect(dialog.getByRole("img", { name: "Pair a device" })).toBeVisible();
  await expect(dialog.getByRole("img", { name: "Download SoundPush for Android" })).toBeVisible();
  await expectAccessible(page, "onboarding pairing step");
  await dialog.getByRole("button", { name: "Skip" }).click();

  await expect(dialog).toBeHidden();
  await expect(page.getByRole("navigation", { name: "Main" })).toContainText("Studio PC");
  await nav(page, "Settings").click();
  await expect(page.getByRole("switch", { name: "Start with Windows" })).toHaveAttribute("aria-checked", "false");
});

test("home: start a stream, read its connection details, stop it", async ({ page }) => {
  await open(page);
  await expectAccessible(page, "home");

  await page.getByRole("button", { name: /Listen on phone/ }).click();
  const card = page.getByRole("article", { name: "Computer audio → Phone" });
  await expect(card).toBeVisible();
  await expect(page.getByRole("button", { name: /Listen on phone/ })).toHaveAttribute("aria-pressed", "true");

  await card.getByRole("button", { name: "Connection details" }).click();
  await expect(card.getByText("QUIC · Local network · IPv4")).toBeVisible();
  await expect(card.getByText("Where the delay comes from")).toBeVisible();
  await expect(card.getByRole("img", { name: /Encoding 10 ms/ })).toBeVisible();
  await expect(card.getByRole("img", { name: /^Last 5 minutes: latency 52 to 52 ms/ })).toBeVisible();

  // "Mute PC" while sending mutes this computer's own speakers.
  const mute = card.getByRole("switch", { name: "Mute this computer's speakers while sending" });
  await expect(mute).toHaveAttribute("aria-checked", "false");
  await mute.click();
  await expect(mute).toHaveAttribute("aria-checked", "true");
  await expectAccessible(page, "home with connection details");

  await card.getByRole("button", { name: "Stop" }).click();
  await expect(card).toBeHidden();
});

test("pairing dialog shows a code and closes with Escape", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Pair", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Pair a device" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("img", { name: "Pair a device" })).toBeVisible();
  await expectAccessible(page, "pairing dialog");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("devices: open a device and test the connection", async ({ page }) => {
  await open(page);
  await nav(page, "Devices").click();
  await expect(page.getByRole("heading", { name: "Devices", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: /Redmi Note 9 Pro/ }).first().click();
  await expect(page.getByRole("heading", { name: "Redmi Note 9 Pro", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByText("Delay", { exact: true })).toBeVisible();
  await expectAccessible(page, "devices");
});

test("audio page", async ({ page }) => {
  await open(page);
  await nav(page, "Audio").click();
  await expect(page.getByRole("heading", { name: "Audio", level: 1 })).toBeVisible();
  await expectAccessible(page, "audio");
});

test("settings: diagnostics preview shows what is saved and removed", async ({ page }) => {
  await open(page);
  await nav(page, "Settings").click();
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: "Help translate" })).toBeVisible();
  await expectAccessible(page, "settings");

  await page.getByRole("button", { name: "Export diagnostics" }).click();
  const dialog = page.getByRole("dialog", { name: "Export diagnostics" });
  await expect(dialog.getByText("Recent log (2 lines)")).toBeVisible();
  await expect(dialog.getByText("IP addresses removed, device IDs shortened, and pairing code removed")).toBeVisible();
  await dialog.getByText("Show everything that will be saved").click();
  await expect(dialog.getByRole("textbox", { name: "Diagnostics file contents" })).toHaveValue(/SoundPush 0\.1\.0 diagnostics/);
  await expectAccessible(page, "diagnostics preview");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("settings: troubleshooter runs its checks", async ({ page }) => {
  await open(page);
  await nav(page, "Settings").click();
  const topic = page.getByRole("button", { name: "Can't find your phone" });
  await topic.click();
  await expect(topic).toHaveAttribute("aria-expanded", "true");
  const steps = page.getByRole("list", { name: "Can't find your phone" });
  await expect(steps.getByRole("listitem").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Check again" })).toBeEnabled();
  await expectAccessible(page, "troubleshooter");
});

test("keyboard shortcuts switch pages", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Control+2");
  await expect(page.getByRole("heading", { name: "Devices", level: 1 })).toBeVisible();
  await page.keyboard.press("Control+4");
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  await expect(nav(page, "Settings")).toHaveAttribute("aria-current", "page");
});
