// SPDX-License-Identifier: AGPL-3.0-or-later
// Enrollment and passkey sign-in in Chromium against the real server (docs/specs/m0/04-local-accounts.md
// T20; 11-app-shell.md B13 to B17, T14, T19). `just e2e` creates the stack's organization and admin
// with `stepclimb admin create` and hands this file the printed setup link. Chromium's virtual
// authenticator (DevTools protocol, WebAuthn domain) stands in for a device with a screen lock.
import AxeBuilder from "@axe-core/playwright";
import { type Browser, expect, type Page, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];
const EMAIL = "avery@example.test";

/** axe's WCAG 2.2 AA violations on the whole page, one line per node. */
async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

/**
 * A virtual authenticator's credential as DevTools hands it out (WebAuthn.getCredentials). The
 * object is passed back whole, so the fields this type leaves out (the backup flags) travel too.
 */
interface VirtualCredential {
  credentialId: string;
  isResidentCredential: boolean;
  rpId?: string;
  privateKey: string;
  userHandle?: string;
  signCount: number;
}

/** The passkey the first serial test leaves the account with, for the tests after it. */
let passkey: VirtualCredential | undefined;

/** The credential ids a virtual authenticator holds, its credentials, and a way to remove one. */
interface Authenticator {
  ids: () => Promise<string[]>;
  credentials: () => Promise<VirtualCredential[]>;
  remove: (credentialId: string) => Promise<void>;
}

/**
 * A platform authenticator that verifies the user and answers every prompt by itself, holding
 * `credential` when one is given.
 */
async function addVirtualAuthenticator(
  page: Page,
  credential?: VirtualCredential,
): Promise<Authenticator> {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: false });
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  });
  if (credential) await cdp.send("WebAuthn.addCredential", { authenticatorId, credential });
  const credentials = async () =>
    (await cdp.send("WebAuthn.getCredentials", { authenticatorId })).credentials;
  return {
    credentials,
    ids: async () =>
      (await cdp.send("WebAuthn.getCredentials", { authenticatorId })).credentials.map(
        (credential) => credential.credentialId,
      ),
    remove: async (credentialId) => {
      await cdp.send("WebAuthn.removeCredential", { authenticatorId, credentialId });
    },
  };
}

/**
 * Adds a passkey to the signed-in account through `/api/v1/me/passkeys` (04 B13) from the page,
 * as a client of the public API does: the start route's options go to the WebAuthn API, and the
 * credential goes back to the add route. The status of each answer, or the name of the error the
 * browser's ceremony ended with.
 */
async function addPasskeyFromPage(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const started = await fetch("/api/v1/me/passkeys/start", { method: "POST" });
    const { options } = await started.json();
    const publicKey = PublicKeyCredential.parseCreationOptionsFromJSON(options.publicKey);
    let credential: Credential | null;
    try {
      credential = await navigator.credentials.create({ publicKey });
    } catch (error) {
      return `${started.status} ${error instanceof DOMException ? error.name : "error"}`;
    }
    if (!(credential instanceof PublicKeyCredential)) return `${started.status} no credential`;
    const added = await fetch("/api/v1/me/passkeys", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ credential: credential.toJSON(), name: "Second device" }),
    });
    return `${started.status} ${added.status}`;
  });
}

function answerOf(page: Page, path: string) {
  return page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === path && response.request().method() === "POST",
  );
}

/** The setup link `just e2e` printed. Assertions on it compare shapes, never print the token. */
function setupLink(): string {
  const link = process.env.SC_E2E_ENROLL_URL;
  if (!link) throw new Error("SC_E2E_ENROLL_URL is unset: run the tests with `just e2e`");
  return link;
}

/** The page's `/theme.css` link. */
function themeHref(page: Page): Promise<string | null> {
  return page.locator('link[rel="stylesheet"][href^="/theme.css"]').getAttribute("href");
}

/** The brand color the page is drawn with. */
function brandColor(page: Page): Promise<string> {
  return page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--sc-brand").trim(),
  );
}

/** The saved `oklch()` primary at another hue: the same lightness and chroma, another brand. */
function otherHue(primary: unknown): string {
  const value = typeof primary === "string" ? primary : "";
  const hue = /^oklch\([\d.]+ [\d.]+ ([\d.]+)\)$/.exec(value)?.[1];
  if (hue === undefined) throw new Error(`the saved primary has no L C H hue: ${String(primary)}`);
  return value.replace(/[\d.]+\)$/, `${(Number(hue) + 150) % 360})`);
}

interface BrandingAnswer {
  status: number;
  body: { css_hash: string; inputs: Record<string, unknown> };
}

/** Reads or replaces the organization's branding with the page's own `fetch`, as the app would. */
function branding(page: Page, method: "GET" | "PUT", inputs?: unknown): Promise<BrandingAnswer> {
  return page.evaluate(
    async ({ method, inputs }) => {
      const response = await fetch("/api/v1/admin/branding", {
        method,
        headers: inputs === undefined ? {} : { "content-type": "application/json" },
        body: inputs === undefined ? undefined : JSON.stringify(inputs),
      });
      return { status: response.status, body: await response.json() };
    },
    { method, inputs },
  );
}

const TOKEN_FRAGMENT = /^#token=[A-Za-z0-9_-]{43}$/;

// The tests after the first use the link it used up and the passkey it made, so they run in order
// and stop together.
test.describe
  .serial("a setup link", () => {
    test("enroll with a passkey, sign out, and sign in with it again (04 T20)", async ({
      page,
    }) => {
      const link = setupLink();
      const linkUrl = new URL(link);
      expect(linkUrl.pathname).toBe("/enroll");
      expect(
        TOKEN_FRAGMENT.test(linkUrl.hash),
        "the link ends #token= and 43 base64url characters",
      ).toBe(true);
      const authenticator = await addVirtualAuthenticator(page);
      const credentials = async () => (await authenticator.ids()).length;

      // The token stays in the fragment: the page request the browser sends carries none of it.
      const pageRequest = page.waitForRequest((request) => request.resourceType() === "document");
      await page.goto(link);
      expect(new URL((await pageRequest).url()).hash === "", "a request carries no fragment").toBe(
        true,
      );
      await expect(page.getByText(`You will sign in as ${EMAIL}.`)).toBeVisible();
      // The page moved the token out of the address bar before anything can be pressed.
      expect(new URL(page.url()).hash === "", "the address bar shows no fragment").toBe(true);
      expect(new URL(page.url()).pathname).toBe("/enroll");
      expect(await violations(page)).toEqual([]);

      const registered = answerOf(page, "/auth/local/enrollment/passkey");
      await page.getByRole("button", { name: "Create a passkey" }).click();
      expect((await registered).status()).toBe(200);
      expect(await credentials()).toBe(1);

      // Signed in: the shell loads the real `/me`, and the link's entry left the history.
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      await expect(page.getByRole("button", { name: "Avery Example, account menu" })).toBeVisible();
      expect(new URL(page.url()).pathname).toBe("/");
      await expect(page).toHaveTitle("Home · Example Training");

      await page.getByRole("button", { name: "Avery Example, account menu" }).click();
      await page.getByRole("menuitem", { name: "Sign out" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
      expect((await page.request.get("/api/v1/me")).status()).toBe(401);

      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL.toUpperCase());
      await page.getByRole("button", { name: "Continue" }).click();
      await expect(page.getByText(`Signing in as ${EMAIL.toUpperCase()}.`)).toBeVisible();
      expect(page.url()).not.toContain("avery");

      const finished = answerOf(page, "/auth/local/passkey/finish");
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      expect((await finished).status()).toBe(200);
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      expect((await page.request.get("/api/v1/me")).status()).toBe(200);

      await test.step("add a passkey while signed in, then sign in with it alone (04 B13)", async () => {
        const [enrolled] = await authenticator.ids();
        if (enrolled === undefined) throw new Error("the enrolled passkey is missing");
        // The options exclude the account's passkeys, so the device that holds one refuses.
        expect(await addPasskeyFromPage(page)).toBe("200 InvalidStateError");
        // Without it, the device registers a new passkey, the only one left to sign in with.
        await authenticator.remove(enrolled);
        expect(await addPasskeyFromPage(page)).toBe("200 201");
        expect(await credentials()).toBe(1);
        expect((await authenticator.ids())[0]).not.toBe(enrolled);

        await page.getByRole("button", { name: "Avery Example, account menu" }).click();
        await page.getByRole("menuitem", { name: "Sign out" }).click();
        await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
        await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
        await page.getByRole("button", { name: "Continue" }).click();
        const again = answerOf(page, "/auth/local/passkey/finish");
        await page.getByRole("button", { name: "Sign in with a passkey" }).click();
        expect((await again).status()).toBe(200);
        await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      });
      [passkey] = await authenticator.credentials();
    });

    test("the used setup link no longer works", async ({ page }) => {
      await page.goto(setupLink());
      await expect(
        page.getByRole("heading", { level: 1, name: "This setup link does not work" }),
      ).toBeVisible();
      await expect(page.getByRole("link", { name: "Go to sign in" })).toHaveAttribute(
        "href",
        "/sign-in",
      );
    });

    // Spec 11 T14 needs an admin's session, so it signs in with the passkey the first test made.
    test("a branding change reaches the next page load without clearing the cache (11 T14)", async ({
      page,
    }) => {
      expect(passkey, "the first test registered a passkey").toBeDefined();
      await addVirtualAuthenticator(page, passkey);
      await page.goto("/sign-in");
      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();

      const before = { href: await themeHref(page), brand: await brandColor(page) };
      expect(before.href).toMatch(/^\/theme\.css\?v=[0-9a-f]{64}$/);
      const saved = await branding(page, "GET");
      expect(saved.status).toBe(200);
      try {
        const changed = await branding(page, "PUT", {
          primary: otherHue(saved.body.inputs.primary),
        });
        expect(changed.status).toBe(200);
        expect(`/theme.css?v=${changed.body.css_hash}`).not.toBe(before.href);

        // The same browser, its cache as it was: the next page links the new stylesheet and is
        // drawn with it.
        const css = page.waitForResponse((r) => new URL(r.url()).pathname === "/theme.css");
        await page.goto("/");
        await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
        expect(await themeHref(page)).toBe(`/theme.css?v=${changed.body.css_hash}`);
        expect((await css).status()).toBe(200);
        expect(await brandColor(page)).not.toBe(before.brand);
      } finally {
        const restored = await branding(page, "PUT", saved.body.inputs);
        expect(restored.status).toBe(200);
      }
    });
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

for (const theme of themes) {
  for (const viewport of widths) {
    test(`the sign-in steps have no axe violations, ${theme} at ${viewport.width} px (T19)`, async ({
      browser,
    }) => {
      const { context, page } = await themed(browser, theme, viewport);
      // Page loads first: once the server sets the flow cookie, Chromium sends its own cookie
      // header in place of the test's, and the appearance cookie no longer reaches the server.
      await page.goto("/enroll");
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      await expect(
        page.getByRole("heading", { level: 1, name: "This page needs your setup link" }),
      ).toBeVisible();
      expect(await violations(page)).toEqual([]);

      await page.goto("/sign-in");
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      await expect(page.getByRole("textbox", { name: "Email" })).toBeVisible();
      expect(await violations(page)).toEqual([]);

      // A client-side step: the page keeps its theme.
      await page.getByRole("textbox", { name: "Email" }).fill("nobody@example.test");
      const started = answerOf(page, "/auth/local/passkey/start");
      await page.getByRole("button", { name: "Continue" }).click();
      expect((await started).status()).toBe(200);
      await expect(page.getByRole("button", { name: "Sign in with a passkey" })).toBeEnabled();
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      expect(await violations(page)).toEqual([]);
      await context.close();
    });
  }
}

for (const viewport of widths) {
  test(`a dead setup link has no axe violations at ${viewport.width} px (T19)`, async ({
    browser,
  }) => {
    const { context, page } = await themed(browser, "light", viewport);
    await page.goto("/enroll#token=not-a-real-token");
    await expect(page.locator("html")).toHaveAttribute("data-sc-theme", "light");
    await expect(
      page.getByRole("heading", { level: 1, name: "This setup link does not work" }),
    ).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await context.close();
  });
}
