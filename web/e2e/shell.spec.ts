// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-in shell in the browser (docs/specs/m0/11-app-shell.md T15, T17). The e2e stack has
// no organization and no sign-in yet (spec 11 I5), so these tests answer `/api/v1/me` in the
// browser with a synthetic user; everything else is the built binary serving the built SPA.
import { expect, type Page, test } from "@playwright/test";

const orgId = "01900000-0000-7000-8000-000000000001";
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
  organization: { id: orgId, slug: "example", name: "Example Training" },
  grants: [],
  features: [],
  appearance: { theme: "system", density: "comfortable" },
};

async function signIn(page: Page) {
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: me }));
}

test("at 1280 px the SideNav shows and the BottomTabs do not (T15)", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await signIn(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
  await expect(page.locator('[data-slot="side-nav"]')).toBeVisible();
  await expect(page.locator('[data-slot="bottom-tabs"]')).toBeHidden();
  await expect(page.getByRole("navigation", { name: "Main" })).toHaveCount(1);
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 375, height: 740 }, hasTouch: true, isMobile: true });

  test("at 375 px the BottomTabs show, with 44 px targets, and the SideNav does not (T15)", async ({
    page,
  }) => {
    await signIn(page);
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
    await expect(page.locator('[data-slot="bottom-tabs"]')).toBeVisible();
    await expect(page.locator('[data-slot="side-nav"]')).toBeHidden();
    const tabs = page.locator('[data-slot="bottom-tabs-link"]');
    expect(await tabs.count()).toBeGreaterThan(0);
    for (const tab of await tabs.all()) {
      const box = await tab.boundingBox();
      expect(box?.width).toBeGreaterThanOrEqual(44);
      expect(box?.height).toBeGreaterThanOrEqual(44);
    }
  });
});

test("client navigation focuses the new view's h1 and names it in the title (T17)", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/nowhere");
  await expect(page).toHaveTitle("Page not found · Example Training");

  await page.getByRole("link", { name: "Go to the home page" }).click();

  const heading = page.getByRole("heading", { level: 1, name: "Home" });
  await expect(heading).toBeFocused();
  await expect(page).toHaveTitle("Home · Example Training");
  // A client-side navigation changes the URL without a new document.
  expect(new URL(page.url()).pathname).toBe("/");
  expect(await page.evaluate(() => performance.getEntriesByType("navigation").length)).toBe(1);
});

test("the skip link is the first focusable element and moves focus to main (B6)", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: "Skip to main content" });
  await expect(skip).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("main")).toBeFocused();
});

// Sign-out against the real server: the browser's own fetch passes the CSRF check (it sends
// `Sec-Fetch-Site: same-origin`), and the server answers 204 with or without a session.
test("Sign out posts to the real server and lands on /sign-in (B12)", async ({ page }) => {
  await signIn(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Avery Example, account menu" }).click();
  const signOut = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/auth/sign-out" &&
      response.request().method() === "POST",
  );
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  const response = await signOut;
  // 403 here would be `auth.csrf_refused`: the browser's request did not pass the CSRF check.
  expect(response.status()).toBe(204);
  await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
  expect(new URL(page.url()).pathname).toBe("/sign-in");
});
