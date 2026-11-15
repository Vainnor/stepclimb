// SPDX-License-Identifier: AGPL-3.0-or-later
// The sign-in screen's method list (docs/specs/m0/11-app-shell.md B13, B14, B18, T19). Against the
// real server: the stack's organization offers local sign-in and one provider, `example-id`, whose
// config holds only its label (`just e2e` inserts it), so its start route sends the browser back
// with `?error=auth.provider_unavailable`. That round trip is the provider button's full-page
// navigation and the callback error's display; signing in at a real provider needs the mock
// OpenID Connect server of spec 05 (T16). The states the stack cannot reach are answered in the
// browser with synthetic discovery documents, each checked by axe in light, dark and high contrast
// at 1280 and 375 px, in the default brand the stack serves and in the vZDC example brand.
import { readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { type Browser, expect, type Page, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** axe's WCAG 2.2 AA violations on the whole page, one line per node. */
async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

test("a provider button opens its start route as a page and the refusal comes back (B14, B18)", async ({
  page,
}) => {
  await page.goto("/sign-in?return_to=%2Faccount");
  await expect(page.getByRole("banner")).toContainText("Example Training");
  await expect(page).toHaveTitle("Sign in · Example Training");
  await expect(page.getByRole("textbox", { name: "Email" })).toBeVisible();
  const button = page.getByRole("link", { name: "Sign in with Example ID" });
  await expect(button).toHaveAttribute("href", "/auth/example-id/start?return_to=%2Faccount");

  // A document request, not a fetch: the browser itself follows the server's redirects.
  const start = page.waitForRequest(
    (request) =>
      new URL(request.url()).pathname === "/auth/example-id/start" &&
      request.resourceType() === "document",
  );
  await button.click();
  const response = await (await start).response();
  expect(response?.status()).toBe(303);
  expect(response?.headers().location).toBe("/sign-in?error=auth.provider_unavailable");

  await expect(page.getByRole("alert")).toContainText(
    "Stepclimb could not start the sign-in with that service.",
  );
  // The parameter left the address bar; the message stays.
  await expect.poll(() => new URL(page.url()).search).toBe("");
  await expect(page.getByRole("alert")).toBeVisible();
  expect(await violations(page)).toEqual([]);
});

const themes = ["light", "dark", "high-contrast"] as const;
const widths = [
  { width: 1280, height: 800 },
  { width: 375, height: 740 },
];

/** A browser whose appearance cookie picks `theme`, at `viewport`. */
async function themed(browser: Browser, theme: string, viewport: (typeof widths)[number]) {
  // The cookie goes in as a header: Playwright's cookie API refuses a `__Host-` cookie for this
  // `http` origin.
  const context = await browser.newContext({
    viewport,
    extraHTTPHeaders: { cookie: `__Host-sc_appearance=${theme}.comfortable` },
  });
  return { context, page: await context.newPage() };
}

const providers = [
  { key: "example-id", kind: "oidc", label: "Sign in with Example ID" },
  { key: "regional", kind: "oauth2", label: "Sign in with the Example Regional Training Network" },
  { key: "corp-sso", kind: "oidc", label: null },
];

/** Answers the discovery document in the browser with `sign_in`. */
async function offer(page: Page, signIn: unknown[]) {
  await page.route("**/.well-known/stepclimb", (route) =>
    route.fulfill({
      json: {
        name: "Example Training",
        sign_in: signIn,
        theme: { stylesheet: "http://localhost/theme.css" },
        oauth: {
          issuer: "http://localhost",
          authorization_endpoint: "http://localhost/oauth/authorize",
        },
      },
    }),
  );
}

const local = { key: "local", kind: "local", label: null, methods: ["passkey", "password"] };

const brands = ["default", "vzdc"] as const;

/**
 * Every brand's stylesheet from the token build (`just web-build` builds it before the tests). The
 * stack's server serves the default brand; changing the instance's branding would reach every
 * other test, so the vZDC brand is drawn from the token build's `[data-sc-brand="vzdc"]` rules.
 */
const allBrands = readFileSync(
  new URL("../../packages/tokens/build/css/variables.css", import.meta.url),
  "utf8",
);

/** Serves `brand`'s colors in place of the stack's `/theme.css` (the default brand serves as is). */
async function serveBrand(page: Page, brand: (typeof brands)[number]) {
  if (brand === "default") return;
  await page.route("**/theme.css*", (route) =>
    route.fulfill({ contentType: "text/css", body: allBrands }),
  );
}

/**
 * Marks the loaded page with `brand`, as Storybook does, once `serveBrand` gave it the rules, and
 * checks that the page's brand color changed with it.
 */
async function drawBrand(page: Page, brand: (typeof brands)[number]) {
  if (brand === "default") return;
  const brandColor = () =>
    page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue("--sc-brand").trim(),
    );
  const before = await brandColor();
  await page.evaluate((b) => {
    document.documentElement.dataset.scBrand = b;
  }, brand);
  await expect(page.locator("html")).toHaveAttribute("data-sc-brand", brand);
  expect(await brandColor()).not.toBe(before);
}

for (const brand of brands) {
  for (const theme of themes) {
    for (const viewport of widths) {
      methodListAxe(brand, theme, viewport);
    }
  }
}

function methodListAxe(
  brand: (typeof brands)[number],
  theme: (typeof themes)[number],
  viewport: (typeof widths)[number],
) {
  test(`the method list's states have no axe violations, ${brand} ${theme} at ${viewport.width} px (T19)`, async ({
    browser,
  }) => {
    const { context, page } = await themed(browser, theme, viewport);
    const reroute = async (signIn: unknown[]) => {
      await page.unrouteAll();
      await serveBrand(page, brand);
      await offer(page, signIn);
    };

    await test.step("local sign-in and three providers", async () => {
      await reroute([local, ...providers]);
      await page.goto("/sign-in");
      await drawBrand(page, brand);
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      await expect(page.getByRole("link", { name: "Sign in with corp-sso" })).toBeVisible();
      expect(await violations(page)).toEqual([]);
    });

    await test.step("providers only, with a code the screen does not know", async () => {
      await reroute(providers);
      await page.goto("/sign-in?error=auth.something_new");
      await drawBrand(page, brand);
      await expect(page.getByRole("alert")).toContainText("auth.something_new");
      await expect(page.getByRole("textbox", { name: "Email" })).toHaveCount(0);
      expect(await violations(page)).toEqual([]);
    });

    await test.step("no way to sign in", async () => {
      await reroute([]);
      await page.goto("/sign-in");
      await drawBrand(page, brand);
      await expect(page.getByText(/has not turned on a way to sign in/)).toBeVisible();
      expect(await violations(page)).toEqual([]);
    });

    await test.step("the password step with a refusal", async () => {
      await reroute([{ ...local, methods: ["password"] }]);
      await page.route("**/auth/local/password", (route) =>
        route.fulfill({
          status: 401,
          contentType: "application/problem+json",
          body: JSON.stringify({
            type: "about:blank",
            title: "Unauthorized",
            status: 401,
            code: "auth.credentials_invalid",
          }),
        }),
      );
      await page.goto("/sign-in");
      await drawBrand(page, brand);
      await page.getByRole("textbox", { name: "Email" }).fill("nobody@example.test");
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByLabel("Password").fill("not the password");
      await page.getByRole("button", { name: "Sign in" }).click();
      await expect(page.getByRole("alert")).toContainText("That email and password do not match.");
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      expect(await violations(page)).toEqual([]);
    });

    await test.step("the TOTP step, with a recovery code and a refusal", async () => {
      await page.unroute("**/auth/local/password");
      await page.route("**/auth/local/password", (route) =>
        route.fulfill({ json: { next: "totp" } }),
      );
      await page.route("**/auth/local/totp", (route) =>
        route.fulfill({
          status: 403,
          contentType: "application/problem+json",
          body: JSON.stringify({
            type: "about:blank",
            title: "Forbidden",
            status: 403,
            code: "auth.suspended",
          }),
        }),
      );
      await page.getByLabel("Password").fill("not the password");
      await page.getByRole("button", { name: "Sign in" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Enter your code" })).toBeVisible();
      expect(await violations(page)).toEqual([]);
      await page.getByRole("button", { name: "Use a recovery code instead" }).click();
      await page.getByRole("textbox", { name: /^Recovery code/ }).fill("ABCD-EFGH-IJKL-MNOP");
      await page.getByRole("button", { name: "Sign in" }).click();
      await expect(page.getByRole("alert")).toContainText("Your account is suspended.");
      expect(await violations(page)).toEqual([]);
    });
    await context.close();
  });
}

test.describe("on a phone", () => {
  // Touch, so `pointer: coarse` matches as on a device: base.css sizes inputs for touch only.
  test.use({ viewport: { width: 375, height: 740 }, isMobile: true, hasTouch: true });

  test("inputs are at least 16 px and the provider button 44 px tall on a phone", async ({
    page,
  }) => {
    await offer(page, [{ ...local, methods: ["password"] }, providers[0]]);
    await page.goto("/sign-in");
    // A provider's button is a link, which base.css does not size: ButtonLink does.
    const provider = await page
      .getByRole("link", { name: "Sign in with Example ID" })
      .boundingBox();
    expect(provider?.height).toBeGreaterThanOrEqual(44);
    const email = page.getByRole("textbox", { name: "Email" });
    expect(
      await email.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize)),
    ).toBeGreaterThanOrEqual(16);
    await email.fill("nobody@example.test");
    await page.getByRole("button", { name: "Continue" }).click();
    const password = page.getByLabel("Password");
    expect(
      await password.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize)),
    ).toBeGreaterThanOrEqual(16);
    await page.route("**/auth/local/password", (route) =>
      route.fulfill({ json: { next: "totp" } }),
    );
    await password.fill("not the password");
    await page.getByRole("button", { name: "Sign in" }).click();
    const code = page.getByRole("textbox", { name: /^Code/ });
    await expect(code).toHaveAttribute("inputmode", "numeric");
    expect(
      await code.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize)),
    ).toBeGreaterThanOrEqual(16);
  });
});
