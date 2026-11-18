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
    // The app registers a service worker in its production build (spec 15 B6). It would answer
    // the page's navigations itself, out of reach of `page.route`, so only pwa.spec.ts allows it.
    serviceWorkers: "block",
  },
  // sign-in.spec.ts changes the organization's branding (spec 11 T14), which every page is drawn
  // with, so it runs once every other file has finished.
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      testIgnore: [/sign-in\.spec\.ts$/, /installed\.spec\.ts$/, /page-edit-ios\.spec\.ts$/],
    },
    // Spec 15 T11: the installed-mode layout in WebKit at 375 px; the page editor on a phone.
    {
      name: "webkit",
      use: { ...devices["iPhone 13"], viewport: { width: 375, height: 740 } },
      testMatch: /(installed|page-edit-ios)\.spec\.ts$/,
    },
    {
      name: "sign-in",
      use: { ...devices["Desktop Chrome"] },
      testMatch: /sign-in\.spec\.ts$/,
      dependencies: ["chromium", "webkit"],
    },
  ],
});
