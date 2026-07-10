/**
 * Build-time constants injected by Vite's `define` (see vite.config.ts).
 * `__APP_VERSION__` is replaced with the literal `package.json` version string
 * at build time, so it costs nothing at runtime and can never drift from the
 * package manifest.
 */
declare const __APP_VERSION__: string
