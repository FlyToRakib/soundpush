import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { section, unwrap } from "./release-notes.mjs";

describe("section", () => {
  it("takes one version's notes and stops at the next heading or link list", () => {
    const changelog = "# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-09-18\n\nText.\n\n## [0.0.9]\n\nOld.\n";
    assert.equal(section(changelog, "0.1.0"), "Text.");
    assert.equal(section(changelog, "Unreleased"), "");
    assert.equal(section(changelog, "9.9.9"), null);
  });
});

describe("unwrap", () => {
  it("joins wrapped paragraphs and list items but keeps blocks apart", () => {
    const text = [
      "A paragraph that is",
      "wrapped.",
      "",
      "### Heading",
      "",
      "- **Item** one that",
      "  goes on.",
      "- Item two.",
      "",
      "| a | b |",
      "|---|---|",
    ].join("\n");
    assert.equal(
      unwrap(text),
      ["A paragraph that is wrapped.", "", "### Heading", "", "- **Item** one that goes on.", "- Item two.", "", "| a | b |", "|---|---|"].join(
        "\n",
      ),
    );
  });
});
