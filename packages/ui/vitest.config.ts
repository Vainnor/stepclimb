// SPDX-License-Identifier: AGPL-3.0-or-later
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    // No file-system module cache: it keys a transformed module on its own text, so a test that
    // reads files through an eager `import.meta.glob` (the catalog and status checks) keeps the
    // file list of its first run and misses files added since.
    fsModuleCache: false,
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    // Date messages format in the host's time zone; tests pin it so they pass in every zone.
    env: { TZ: "UTC" },
  },
});
