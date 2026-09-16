// Shared Svelte configuration. The Vite plugin picks it up automatically; eslint-plugin-svelte and
// prettier-plugin-svelte are handed it explicitly so they compile components the same way.
export default {
  compilerOptions: {
    // Svelte 5 runes ($state, $derived, $props) are used throughout.
    runes: true,
  },
};
