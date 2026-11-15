// SPDX-License-Identifier: AGPL-3.0-or-later
// The smoke test runs against the Compose stack (`just e2e` starts it), never a dev server.
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: process.env.SC_E2E_URL ?? "http://localhost:58080",
    trace: "retain-on-failure",
  },
  // sign-in.spec.ts changes the organization's branding (spec 11 T14), which every page is drawn
  // with, so it runs once every other file has finished.
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      testIgnore: /sign-in\.spec\.ts$/,
    },
    {
      name: "sign-in",
      use: { ...devices["Desktop Chrome"] },
      testMatch: /sign-in\.spec\.ts$/,
      dependencies: ["chromium"],
    },
  ],
});
