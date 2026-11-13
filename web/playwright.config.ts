// SPDX-License-Identifier: AGPL-3.0-or-later
// The smoke test runs against the Compose stack (`just e2e` starts it), never a dev server.
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: process.env.SC_E2E_URL ?? "http://127.0.0.1:58080",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
