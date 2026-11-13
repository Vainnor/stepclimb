// SPDX-License-Identifier: AGPL-3.0-or-later
// The story suite (docs/specs/m0/13-base-ui-components.md B24, B29; Q4): every story of the static
// Storybook build in each theme at 1280 and 375 px, compared with a committed screenshot and
// checked with axe. It runs inside Playwright's pinned image (`bin/ui-visual.sh`, `just
// ui-visual`) so a baseline renders the same on every machine; on the host, fonts and Chromium
// differ and the screenshots do not match.
import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.SC_STORYBOOK_PORT ?? "6007");

export default defineConfig({
  testDir: "visual",
  outputDir: "test-results",
  snapshotPathTemplate: "{testDir}/baselines/{arg}--{projectName}{ext}",
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  reporter: [["list"]],
  expect: {
    // Exact pixels: the pinned image renders the same pixels each run, and Playwright's default
    // per-pixel threshold (0.2) lets a 1 px radius change on a filled button through.
    toHaveScreenshot: { threshold: 0, maxDiffPixels: 0, animations: "disabled", caret: "hide" },
  },
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  // A test tagged @phone runs only in the 375 px project, one tagged @desktop only at 1280 px.
  projects: [
    {
      name: "1280",
      grepInvert: /@phone/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    {
      // A phone: 375 px wide with touch, so `pointer: coarse` matches as it does on a device.
      name: "375",
      grepInvert: /@desktop/,
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 375, height: 812 },
        isMobile: true,
        hasTouch: true,
        deviceScaleFactor: 1,
      },
    },
  ],
  webServer: {
    command: "node visual/serve.mjs",
    url: `http://127.0.0.1:${port}/index.json`,
    reuseExistingServer: false,
  },
});
