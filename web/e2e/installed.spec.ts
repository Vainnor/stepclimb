// SPDX-License-Identifier: AGPL-3.0-or-later
// The installable app's page in WebKit at 375 px
// (docs/specs/m1/15-installable-web-app-and-offline-reading.md B2, T11): the links an installed
// app reads, and the fixed BottomTabs inside the screen with their safe-area padding intact.
// Playwright's WebKit cannot report safe-area insets or run from the home screen, so the iOS
// home-screen check is by hand and the PR says whether it ran. `/api/v1/me` is answered in the
// browser with a synthetic user, as in shell.spec.ts.
import { expect, test } from "./csp";

const me = {
  user: {
    id: "01900000-0000-7000-8000-000000000002",
    display_name: "Avery Example",
    given_name: "Avery",
    family_name: "Example",
    email: "avery@example.test",
    locale: "en",
    tz: "UTC",
    status: "active",
    source: "local",
  },
  organization: {
    id: "01900000-0000-7000-8000-000000000001",
    slug: "example",
    name: "Example Training",
  },
  grants: [],
  features: [],
  appearance: { theme: "system", density: "comfortable" },
};

test("the page links what an installed app needs and keeps its fixed bar inside the safe area (T11)", async ({
  page,
}) => {
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: me }));
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();

  await expect(page.locator('meta[name="viewport"]')).toHaveAttribute(
    "content",
    /viewport-fit=cover/,
  );
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
    "content",
    /^#[0-9a-f]{6}$/,
  );
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute(
    "href",
    "/manifest.webmanifest",
  );
  const touchIcon = await page.locator('link[rel="apple-touch-icon"]').getAttribute("href");
  const icon = await page.request.get(touchIcon ?? "");
  expect(icon.status()).toBe(200);
  expect(icon.headers()["content-type"]).toBe("image/png");

  const tabs = page.locator('[data-slot="bottom-tabs"]');
  await expect(tabs).toBeVisible();
  const box = await tabs.boundingBox();
  const height = await page.evaluate(() => window.innerHeight);
  expect(box?.y).toBeGreaterThanOrEqual(0);
  expect((box?.y ?? Number.POSITIVE_INFINITY) + (box?.height ?? 0)).toBeLessThanOrEqual(height);
  // WebKit kept the rules that pad the bar by the bottom and side safe areas: a rule it could not
  // parse would be dropped from its stylesheet.
  const insets = await tabs.evaluate((element) => {
    const found = new Set<string>();
    const matches = (selector: string) => {
      try {
        return element.matches(selector);
      } catch {
        return false; // a pseudo-element selector, which no element matches
      }
    };
    const walk = (rules: CSSRuleList) => {
      for (const rule of Array.from(rules)) {
        if (rule instanceof CSSStyleRule && matches(rule.selectorText)) {
          for (const side of ["bottom", "left", "right"]) {
            if (rule.cssText.includes(`env(safe-area-inset-${side})`)) found.add(side);
          }
        }
        if ("cssRules" in rule) walk((rule as CSSGroupingRule).cssRules);
      }
    };
    for (const sheet of Array.from(document.styleSheets)) walk(sheet.cssRules);
    return [...found].sort();
  });
  expect(insets).toEqual(["bottom", "left", "right"]);
});
