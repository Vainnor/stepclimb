// SPDX-License-Identifier: AGPL-3.0-or-later
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // No file-system module cache: it keys a transformed module on its own text, so a test that
    // reads files through an eager `import.meta.glob` (the catalog and status checks) keeps the
    // file list of its first run and misses files added since.
    fsModuleCache: false,
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
