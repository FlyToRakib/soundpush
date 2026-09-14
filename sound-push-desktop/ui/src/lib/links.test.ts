import { describe, expect, it, vi } from "vitest";
import { DOCS_SITE, LINKS, docsUrl } from "./links";

describe("docsUrl", () => {
  it("uses the documentation site when it answers", async () => {
    const fetcher = vi.fn(async () => new Response(null, { status: 200 }));
    await expect(docsUrl("userGuide", fetcher)).resolves.toBe(`${DOCS_SITE}/user-guide/`);
    expect(fetcher).toHaveBeenCalledWith(`${DOCS_SITE}/user-guide/`, expect.objectContaining({ method: "HEAD" }));
  });

  it("falls back to GitHub when the site is missing", async () => {
    const fetcher = vi.fn(async () => new Response(null, { status: 404 }));
    await expect(docsUrl("privacy", fetcher)).resolves.toBe(LINKS.privacy);
  });

  it("falls back to GitHub when offline or blocked", async () => {
    const fetcher = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });
    await expect(docsUrl("troubleshooting", fetcher)).resolves.toBe(`${LINKS.userGuide}#9-troubleshooting`);
  });
});
