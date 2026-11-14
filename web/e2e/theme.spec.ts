// SPDX-License-Identifier: AGPL-3.0-or-later
// The theme the server writes into the page (docs/specs/m0/11-app-shell.md B20, B21, Q4, Q6; T13).
// The e2e stack has no organization, so `/theme.css` is the default brand's; the appearance
// comes from the `__Host-sc_appearance` cookie, as for any signed-out browser. The menu test
// answers `/api/v1/me` and the save in the browser, as tests/shell.spec.ts does.
import { type Browser, expect, type Page, test } from "@playwright/test";

/** A page opened with JavaScript off, so only what the server sent can draw it. */
async function withoutScript(browser: Browser, appearance?: string): Promise<Page> {
  // The cookie goes in as a header: Playwright's cookie API refuses a `__Host-` cookie for this
  // `http` origin.
  const headers: Record<string, string> = appearance
    ? { cookie: `__Host-sc_appearance=${appearance}` }
    : {};
  const context = await browser.newContext({ javaScriptEnabled: false, extraHTTPHeaders: headers });
  const page = await context.newPage();
  await page.goto("/sign-in", { waitUntil: "domcontentloaded" });
  return page;
}

const canvas = (page: Page) =>
  page.evaluate(() => getComputedStyle(document.documentElement).backgroundColor);

test("the page links the fingerprinted theme before its first module script (T13)", async ({
  request,
}) => {
  const html = await (await request.get("/")).text();
  const link = html.match(/<link rel="stylesheet" href="(\/theme\.css\?v=[0-9a-f]{64})"/);
  expect(link, html).not.toBeNull();
  const href = link?.[1] ?? "";
  expect(html.indexOf(href)).toBeLessThan(html.indexOf('<script type="module"'));

  const css = await request.get(href);
  expect(css.status()).toBe(200);
  expect(css.headers()["cache-control"]).toBe("public, max-age=31536000, immutable");
  expect(css.headers()["content-type"]).toBe("text/css; charset=utf-8");
  expect(await css.text()).toContain("--sc-bg-canvas:");
});

test("without JavaScript the page already has the theme's canvas color (T13)", async ({
  browser,
}) => {
  const page = await withoutScript(browser);
  expect(await page.locator("html").getAttribute("data-sc-theme")).toBeNull();
  // The bundle declares no color: the canvas variable comes from /theme.css alone, and the page
  // is painted with it.
  const declared = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--sc-bg-canvas").trim(),
  );
  expect(declared).toMatch(/^oklch\(/);
  const token = await page.evaluate(() => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = "var(--sc-bg-canvas)";
    document.body.append(probe);
    return getComputedStyle(probe).backgroundColor;
  });
  expect(await canvas(page)).toBe(token);
});

test("a signed-out browser's appearance cookie is in the page before any script (B21, Q6)", async ({
  browser,
}) => {
  const plain = await canvas(await withoutScript(browser));
  const page = await withoutScript(browser, "dark.compact");
  const root = page.locator("html");
  expect(await root.getAttribute("data-sc-theme")).toBe("dark");
  expect(await root.getAttribute("data-sc-density")).toBe("compact");
  expect(await canvas(page)).not.toBe(plain);
});

test("choosing a theme in the user menu redraws the page in it (B21)", async ({ page }) => {
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
    organization: { id: "01900000-0000-7000-8000-000000000001", slug: "example", name: "Example" },
    grants: [],
    features: [],
    appearance: { theme: "light", density: "comfortable" },
  };
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: me }));
  await page.route("**/api/v1/me/appearance", (route) =>
    route.fulfill({ json: route.request().postDataJSON() }),
  );
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-sc-theme", "light");
  const light = await canvas(page);

  await page.getByRole("button", { name: "Avery Example, account menu" }).click();
  await page.getByRole("menuitem", { name: "Theme" }).click();
  await page.getByRole("menuitemradio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-sc-theme", "dark");
  expect(await canvas(page)).not.toBe(light);
});
