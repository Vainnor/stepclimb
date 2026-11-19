// SPDX-License-Identifier: AGPL-3.0-or-later
// The shell's states in the browser (docs/specs/m0/11-app-shell.md B27, B29, B30, T19): the
// signed-in home with its empty state in light, dark and high contrast at 1280 and 375 px, the
// not-found view in both layouts, the server that does not answer, and the offline notice. As in
// shell.spec.ts, `/api/v1/me` is answered in the browser with a synthetic user; the rest is the
// built binary serving the built SPA.
import AxeBuilder from "@axe-core/playwright";
import { type Browser, expect, type Page, type Route, test } from "./csp";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** axe's WCAG 2.2 AA violations on the whole page, one line per node. */
async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

const themes = ["light", "dark", "high-contrast"] as const;
const widths = [
  { width: 1280, height: 800 },
  { width: 375, height: 740 },
];

function meWith(theme: string) {
  return {
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
    appearance: { theme, density: "comfortable" },
    default_appearance: "system",
  };
}

/** Answers `/api/v1/me` with the synthetic user while `up()` is true; aborts it otherwise. */
async function serveMe(page: Page, theme = "light", up: () => boolean = () => true) {
  await page.route("**/api/v1/me", (route: Route) =>
    up() ? route.fulfill({ json: meWith(theme) }) : route.abort("internetdisconnected"),
  );
}

const home = (page: Page) => page.getByRole("heading", { level: 1, name: "Home" });

for (const theme of themes) {
  for (const viewport of widths) {
    test(`the signed-in home has no axe violations, ${theme} at ${viewport.width} px (T19, B30)`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await serveMe(page, theme);
      await page.goto("/");
      await expect(home(page)).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      await expect(page.locator('[data-slot="empty-state"]')).toHaveText(
        "You have no courses yet.",
      );
      expect(await violations(page)).toEqual([]);
    });
  }
}

for (const viewport of widths) {
  test(`not found has no axe violations in either layout at ${viewport.width} px (B27)`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    // Signed out: the real server answers `/me` with 401.
    await page.goto("/nowhere");
    await expect(
      page.getByRole("heading", { level: 1, name: "There is no page at /nowhere" }),
    ).toBeVisible();
    await expect(page.locator('[data-slot="app-shell"]')).toHaveCount(0);
    expect(await violations(page)).toEqual([]);

    await serveMe(page);
    await page.goto("/nowhere");
    await expect(page.getByRole("heading", { level: 1, name: "Page not found" })).toBeVisible();
    await expect(
      page.getByRole("heading", { level: 2, name: "There is no page at /nowhere" }),
    ).toBeVisible();
    await expect(page.locator('[data-slot="app-shell"]')).toHaveCount(1);
    expect(await violations(page)).toEqual([]);
  });
}

/**
 * A browser at `viewport` whose appearance cookie picks `theme`, so the page the server writes is
 * drawn in it before `/me` answers, or when it never does.
 */
async function themed(browser: Browser, theme: string, viewport: (typeof widths)[number]) {
  // The cookie goes in as a header: Playwright's cookie API refuses a `__Host-` cookie for this
  // `http` origin.
  const context = await browser.newContext({
    viewport,
    extraHTTPHeaders: { cookie: `__Host-sc_appearance=${theme}.comfortable` },
  });
  return { context, page: await context.newPage() };
}

for (const theme of themes) {
  for (const viewport of widths) {
    test(`a server that does not answer is said so, and Try again loads the page, ${theme} at ${viewport.width} px (B29)`, async ({
      browser,
    }) => {
      const { context, page } = await themed(browser, theme, viewport);
      let up = false;
      await serveMe(page, theme, () => up);
      await page.goto("/");
      await expect(
        page.getByRole("heading", { level: 1, name: "The server could not be reached" }),
      ).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      expect(new URL(page.url()).pathname).toBe("/");
      expect(await violations(page)).toEqual([]);

      up = true;
      await page.getByRole("button", { name: "Try again" }).click();
      await expect(home(page)).toBeVisible();
      await expect(home(page)).toBeFocused();
      await context.close();
    });

    test(`offline, the shell keeps the page and says the server could not be reached, ${theme} at ${viewport.width} px (B29)`, async ({
      browser,
    }) => {
      const { context, page } = await themed(browser, theme, viewport);
      let up = true;
      await serveMe(page, theme, () => up);
      await page.goto("/");
      await expect(home(page)).toBeVisible();

      up = false;
      await context.setOffline(true);
      // A tab coming back into view refetches what it shows (TanStack Query listens on `window`).
      await page.evaluate(() => window.dispatchEvent(new Event("visibilitychange")));
      const notice = page.locator('[data-slot="unreachable-notice"]');
      await expect(notice).toContainText("The server could not be reached");
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      await expect(home(page)).toBeVisible();
      await expect(page.getByRole("button", { name: "Avery Example, account menu" })).toBeVisible();
      expect(new URL(page.url()).pathname).toBe("/");
      expect(await violations(page)).toEqual([]);

      // The network returns: the page asks again by itself and the notice goes.
      up = true;
      await context.setOffline(false);
      await expect(notice).toBeEmpty();
      await context.close();
    });
  }
}
