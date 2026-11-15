// SPDX-License-Identifier: AGPL-3.0-or-later
// The web app. `pnpm --filter web dev` serves it on :5173 and proxies the server's own paths to
// `stepclimb serve` (SC_DEV_SERVER, default http://127.0.0.1:8080), so the page talks to
// the API on the same origin and takes its theme from the server's `/theme.css`, as it does when
// the binary serves the built app (docs/specs/m0/11-app-shell.md B20, B22).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

const server = process.env.SC_DEV_SERVER ?? "http://127.0.0.1:8080";

/** The paths the server answers, which the dev server passes on to it. */
export const proxied = [
  "/healthz",
  "/readyz",
  "/theme.css",
  "/.well-known/stepclimb",
  "/auth",
  "/api",
  "/docs",
];

/**
 * The default brand's stylesheet as `dist/theme.css`, for a build that runs without a server (the
 * Tauri shell). The server never serves it: its `/theme.css` route answers before the built files.
 */
function defaultTheme(): Plugin {
  return {
    name: "stepclimb-default-theme",
    apply: "build",
    generateBundle() {
      const file = fileURLToPath(import.meta.resolve("@stepclimb/tokens/theme.css"));
      this.emitFile({ type: "asset", fileName: "theme.css", source: readFileSync(file, "utf8") });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), defaultTheme()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: Object.fromEntries(proxied.map((p) => [p, server])),
  },
  build: { outDir: "dist", emptyOutDir: true },
  test: {
    // No file-system module cache: it keys a transformed module on its own text, so a test that
    // reads files through an eager `import.meta.glob` (the catalog and status checks) keeps the
    // file list of its first run and misses files added since.
    fsModuleCache: false,
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test-setup.ts"],
  },
});
