// node --test tools/ci/*.test.mjs
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { gate, parseAllowlist, warningsFrom } from "./clippy-gate.mjs";

const warning = (lint, file, level = "warning") =>
  JSON.stringify({
    reason: "compiler-message",
    message: { level, code: { code: lint }, rendered: `${level}: ${lint}`, spans: [{ is_primary: true, file_name: file }] },
  });

describe("clippy gate", () => {
  it("reads lint and file from cargo JSON, normalising Windows paths", () => {
    const warnings = warningsFrom([
      warning("clippy::result_unit_err", "sound-push-core\\sp-engine\\src\\pipeline\\sender.rs"),
      warning("clippy::result_unit_err", "sound-push-core/sp-engine/src/pipeline/sender.rs"),
      warning("E0308", "a.rs", "error"),
      JSON.stringify({ reason: "compiler-artifact" }),
      "not json",
    ]);
    assert.deepEqual([...warnings.keys()], ["clippy::result_unit_err sound-push-core/sp-engine/src/pipeline/sender.rs"]);
  });

  it("passes allowlisted warnings and fails new ones", () => {
    const allowed = parseAllowlist("# comment\nclippy::a x.rs\n\nclippy::b y.rs  # trailing\n");
    const warnings = warningsFrom([warning("clippy::a", "x.rs"), warning("clippy::c", "z.rs")]);
    assert.deepEqual(gate(warnings, allowed), { fresh: ["clippy::c z.rs"], fixed: ["clippy::b y.rs"] });
  });

  it("behaves like -D warnings with an empty allowlist", () => {
    const warnings = warningsFrom([warning("clippy::a", "x.rs")]);
    assert.deepEqual(gate(warnings, parseAllowlist("")).fresh, ["clippy::a x.rs"]);
    assert.deepEqual(gate(new Map(), parseAllowlist("")), { fresh: [], fixed: [] });
  });
});
