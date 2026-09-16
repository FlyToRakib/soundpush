// ESLint flat config for the desktop UI (plan §29.3: "eslint/svelte-check" on every PR).
// Formatting is Prettier's job — `svelte/flat/prettier` switches off every rule that would argue
// with it — so what is left here is correctness: unused code, unsafe patterns, Svelte mistakes.
import js from "@eslint/js";
import svelte from "eslint-plugin-svelte";
import globals from "globals";
import tseslint from "typescript-eslint";
import svelteConfig from "./svelte.config.js";

export default tseslint.config(
  {
    ignores: ["dist/", "playwright-report/", "test-results/", "src/lib/engine/bindings.ts"],
  },
  js.configs.recommended,
  tseslint.configs.recommended,
  svelte.configs.recommended,
  svelte.configs.prettier,
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.es2022 },
    },
    rules: {
      // An unused argument kept for a signature is fine when it is marked with a leading underscore.
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
      // The engine's IPC surface is typed by hand in lib/engine/types.ts; `any` is never needed.
      "@typescript-eslint/no-explicit-any": "error",
      "no-console": ["error", { allow: ["warn", "error"] }],
      eqeqeq: ["error", "always", { null: "ignore" }],
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: { parser: tseslint.parser, svelteConfig },
    },
  },
  {
    // Node, not the browser: the Playwright suite and the Vite config.
    files: ["e2e/**/*.ts", "*.config.ts", "*.config.js"],
    languageOptions: { globals: globals.node },
  },
);
