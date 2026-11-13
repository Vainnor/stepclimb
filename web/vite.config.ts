// SPDX-License-Identifier: AGPL-3.0-or-later
// The web app. `pnpm --filter web dev` serves it on :5173 and proxies the server's own paths to
// `stepclimb serve` (SC_DEV_SERVER, default http://127.0.0.1:8080), so the page talks to
// the API on the same origin, as it does when the binary serves the built app.
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const server = process.env.SC_DEV_SERVER ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: Object.fromEntries(["/healthz", "/readyz", "/api", "/docs"].map((p) => [p, server])),
  },
  build: { outDir: "dist", emptyOutDir: true },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test-setup.ts"],
  },
});
