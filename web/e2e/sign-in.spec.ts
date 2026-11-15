// SPDX-License-Identifier: AGPL-3.0-or-later
// Enrollment and passkey sign-in in Chromium against the real server (docs/specs/m0/04-local-accounts.md
// T20; 11-app-shell.md B13 to B17, T14, T16, T19), and sign-in at the mock OpenID Connect server
// the stack runs (06-oauth2-and-vatsim-preset.md B3, T19). `just e2e` creates the stack's organization and admin
// with `stepclimb admin create` and hands this file the printed setup link. Chromium's virtual
// authenticator (DevTools protocol, WebAuthn domain) stands in for a device with a screen lock.
import { createHmac } from "node:crypto";
import AxeBuilder from "@axe-core/playwright";
import { type Browser, expect, type Page, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];
const EMAIL = "avery@example.test";
/** The user the password test adds, and the passphrase they set: a throwaway stack's test data. */
const PASSWORD_USER = "blake@example.test";
const PHRASE = "plinth harbor lantern 7342";
/** The user the required-TOTP test adds; they set the same passphrase. */
const SET_UP_USER = "casey@example.test";

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
      const authenticator = await addVirtualAuthenticator(page, passkey);
      await page.goto("/sign-in");
      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      // The sign-in raised the passkey's sign counter: the next test signs in with this copy.
      [passkey] = await authenticator.credentials();

      const before = { href: await themeHref(page), brand: await brandColor(page) };
      expect(before.href).toMatch(/^\/theme\.css\?v=[0-9a-f]{64}$/);
      const saved = await branding(page, "GET");
      expect(saved.status).toBe(200);
      try {
        // The admin has chosen no theme, so the organization's default appearance draws the page
        // (12 B27).
        expect(await page.locator("html").getAttribute("data-sc-theme")).toBeNull();
        const changed = await branding(page, "PUT", {
          primary: otherHue(saved.body.inputs.primary),
          default_appearance: "dark",
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
        await expect(page.locator("html")).toHaveAttribute("data-sc-theme", "dark");
      } finally {
        const restored = await branding(page, "PUT", saved.body.inputs);
        expect(restored.status).toBe(200);
      }
    });

    // The account page against the real server (04 B13): the first test left the account two
    // passkeys, "Passkey" from the setup link and "Second device", which the authenticator holds.
    test("add, rename and delete passkeys on the account page, and keep the last one", async ({
      page,
    }) => {
      expect(passkey, "the first test registered a passkey").toBeDefined();
      const authenticator = await addVirtualAuthenticator(page, passkey);
      await page.goto("/sign-in");
      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();

      await page.getByRole("link", { name: "Account", exact: true }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Account" })).toBeVisible();
      await expect(page).toHaveTitle("Account · Example Training");
      const rows = page.getByRole("row");
      await expect(rows).toHaveCount(2);
      await expect(rows.nth(0)).toContainText("Passkey");
      await expect(rows.nth(1)).toContainText("Second device");
      await expect(rows.nth(1)).toContainText("Last used");

      await test.step("add: the device that holds one of the account's passkeys refuses", async () => {
        await page.getByRole("button", { name: "Add a passkey" }).click();
        const dialog = page.getByRole("dialog", { name: "Add a passkey" });
        await dialog.getByRole("textbox", { name: "Name" }).fill("Tablet");
        await dialog.getByRole("button", { name: "Create the passkey" }).click();
        await expect(dialog.getByRole("alert")).toContainText(
          "This device already holds a passkey for your account.",
        );
        await dialog.getByRole("button", { name: "Cancel" }).click();
        await expect(dialog).toBeHidden();
      });

      await test.step("add: a device without one registers a new passkey", async () => {
        const [held] = await authenticator.ids();
        if (held === undefined) throw new Error("the authenticator holds no passkey");
        await authenticator.remove(held);
        await page.getByRole("button", { name: "Add a passkey" }).click();
        const dialog = page.getByRole("dialog", { name: "Add a passkey" });
        await dialog.getByRole("textbox", { name: "Name" }).fill("Tablet");
        const added = page.waitForResponse(
          (r) =>
            new URL(r.url()).pathname === "/api/v1/me/passkeys" && r.request().method() === "POST",
        );
        await dialog.getByRole("button", { name: "Create the passkey" }).click();
        expect((await added).status()).toBe(201);
        await expect(dialog).toBeHidden();
        await expect(rows).toHaveCount(3);
        await expect(rows.nth(2)).toContainText("Tablet");
        await expect(rows.nth(2)).toContainText("Not used to sign in yet");
        expect(await authenticator.ids()).toHaveLength(1);
      });

      await test.step("rename", async () => {
        await page.getByRole("button", { name: "Rename Tablet" }).click();
        const dialog = page.getByRole("dialog", { name: "Rename passkey" });
        await dialog.getByRole("textbox", { name: "Name" }).fill("Work tablet");
        await dialog.getByRole("button", { name: "Save name" }).click();
        await expect(dialog).toBeHidden();
        await expect(rows.nth(2)).toContainText("Work tablet");
        await page.reload();
        await expect(page.getByRole("row").nth(2)).toContainText("Work tablet");
      });

      await test.step("delete asks first; Cancel keeps the passkey", async () => {
        await page.getByRole("button", { name: "Delete Passkey" }).click();
        const confirm = page.getByRole("alertdialog", { name: "Delete Passkey?" });
        await confirm.getByRole("button", { name: "Cancel" }).click();
        await expect(confirm).toBeHidden();
        await expect(rows).toHaveCount(3);
      });

      await test.step("delete two once confirmed", async () => {
        for (const name of ["Passkey", "Second device"]) {
          await page.getByRole("button", { name: `Delete ${name}` }).click();
          const confirm = page.getByRole("alertdialog", { name: `Delete ${name}?` });
          const deleted = page.waitForResponse((r) => r.request().method() === "DELETE");
          // By keyboard, so the browser would draw a ring on what takes focus next.
          await confirm.getByRole("button", { name: "Delete passkey" }).press("Enter");
          expect((await deleted).status()).toBe(204);
          await expect(page.getByText(`${name} deleted`)).toBeVisible();
          // The deleted row held focus; the section's heading takes it, without a ring.
          const heading = page.getByRole("heading", { level: 2, name: "Passkeys" });
          await expect(heading).toBeFocused();
          expect(await heading.evaluate((h) => getComputedStyle(h).boxShadow)).toBe("none");
        }
        await expect(rows).toHaveCount(1);
        await expect(rows.nth(0)).toContainText("Work tablet");
      });

      await test.step("the last way to sign in cannot be deleted", async () => {
        await page.getByRole("button", { name: "Delete Work tablet" }).click();
        const confirm = page.getByRole("alertdialog", { name: "Delete Work tablet?" });
        const refused = page.waitForResponse((r) => r.request().method() === "DELETE");
        await confirm.getByRole("button", { name: "Delete passkey" }).click();
        expect((await refused).status()).toBe(409);
        // The page's own alert, not a toast's.
        await expect(page.locator('[data-slot="form-error"]')).toContainText(
          "This passkey is your last way to sign in, so it cannot be deleted.",
        );
        await page.reload();
        await expect(page.getByRole("row")).toHaveCount(1);
      });
      [passkey] = await authenticator.credentials();
    });

    // Password sign-in (04 B14) end to end: the admin adds a user and makes them a setup link
    // through the public API, the link sets a password, and the user signs in with it from the
    // sign-in screen: a wrong password first, then the right one.
    test("sign in with a password from the sign-in screen (04 B14, 11 B16)", async ({
      page,
      browser,
    }) => {
      expect(passkey, "an earlier test left the admin a passkey").toBeDefined();
      const authenticator = await addVirtualAuthenticator(page, passkey);
      await page.goto("/sign-in");
      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      [passkey] = await authenticator.credentials();

      const link = await page.evaluate(async (email) => {
        const created = await fetch("/api/v1/users", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ display_name: "Blake Example", email }),
        });
        const user = await created.json();
        const issued = await fetch(`/api/v1/admin/users/${user.id}/enrollment-links`, {
          method: "POST",
        });
        return { created: created.status, issued: issued.status, ...(await issued.json()) };
      }, PASSWORD_USER);
      expect([link.created, link.issued]).toEqual([201, 201]);

      const context = await browser.newContext();
      const learner = await context.newPage();
      try {
        // The link's token, set through the enrollment route as a client of the API would.
        await learner.goto("/sign-in");
        const token = new URL(link.url).hash.replace(/^#token=/, "");
        const enrolled = await learner.evaluate(
          async ({ token, password }) =>
            (
              await fetch("/auth/local/enrollment/password", {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: JSON.stringify({ token, password }),
              })
            ).status,
          { token, password: PHRASE },
        );
        expect(enrolled).toBe(200);
        await context.clearCookies();

        await learner.goto("/sign-in?return_to=%2Faccount");
        await learner.getByRole("textbox", { name: "Email" }).fill(PASSWORD_USER);
        await learner.getByRole("button", { name: "Continue" }).click();
        // Passkeys come first; the password is the user's choice.
        await learner.getByRole("link", { name: "Sign in with a password instead" }).click();
        await expect(
          learner.getByRole("heading", { level: 1, name: "Enter your password" }),
        ).toBeVisible();

        const refused = answerOf(learner, "/auth/local/password");
        await learner.getByLabel("Password").fill(`${PHRASE}-wrong`);
        await learner.getByRole("button", { name: "Sign in" }).click();
        expect((await refused).status()).toBe(401);
        await expect(learner.getByRole("alert")).toContainText(
          "That email and password do not match.",
        );

        const accepted = answerOf(learner, "/auth/local/password");
        await learner.getByLabel("Password").fill(PHRASE);
        await learner.getByRole("button", { name: "Sign in" }).click();
        expect((await accepted).status()).toBe(200);
        await expect(learner.getByRole("heading", { level: 1, name: "Account" })).toBeVisible();
        expect(new URL(learner.url()).pathname).toBe("/account");
        await expect(
          learner.getByRole("button", { name: "Blake Example, account menu" }),
        ).toBeVisible();
        expect(learner.url()).not.toContain("lantern");
      } finally {
        await context.close();
      }
    });

    // An app's sign-in (08 B31, B45, T29) in a real browser: the authorization request without a
    // session goes through the sign-in screen, which loads the request again as a page; the
    // confirmation form posts with the page's origin (its Referrer-Policy), and Allow sends the
    // browser to the app's loopback URI with a code, the state and the issuer.
    test("an app's sign-in reaches its loopback redirect with a code (08 T29)", async ({
      browser,
    }) => {
      const context = await browser.newContext();
      const learner = await context.newPage();
      const redirect = "http://127.0.0.1:43210/oauth/callback";
      try {
        const query = new URLSearchParams({
          response_type: "code",
          client_id: "stepclimb-app",
          redirect_uri: redirect,
          code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
          code_challenge_method: "S256",
          state: "e2e-state",
        });
        await learner.goto(`/oauth/authorize?${query}`);
        expect(new URL(learner.url()).pathname).toBe("/sign-in");
        await learner.getByRole("textbox", { name: "Email" }).fill(PASSWORD_USER);
        await learner.getByRole("button", { name: "Continue" }).click();
        await learner.getByRole("link", { name: "Sign in with a password instead" }).click();
        await learner.getByLabel("Password").fill(PHRASE);
        await learner.getByRole("button", { name: "Sign in" }).click();
        await expect(
          learner.getByRole("heading", { level: 1, name: "Sign in to the Stepclimb app?" }),
        ).toBeVisible();

        const issuer = new URL(learner.url()).origin;
        // Playwright does not route a request a redirect makes, so the browser's request to the
        // loopback address is only observed; it fails, as nothing listens there.
        const arrived = learner.waitForRequest((r) => r.url().startsWith(`${redirect}?`));
        const answered = learner.waitForResponse(
          (r) =>
            r.request().method() === "POST" && new URL(r.url()).pathname === "/oauth/authorize",
        );
        await learner.getByRole("button", { name: "Allow" }).click();
        const answer = await answered;
        expect(answer.status()).toBe(303);
        const target = new URL((await arrived).url());
        expect(`${target.origin}${target.pathname}`).toBe(redirect);
        expect(target.searchParams.get("code")).toMatch(/^[A-Za-z0-9_-]{43}$/);
        expect(target.searchParams.get("state")).toBe("e2e-state");
        expect(target.searchParams.get("iss")).toBe(issuer);
      } finally {
        await context.close();
      }
    });

    // TOTP after the password (04 B15, B19 to B21) end to end: the password test's user turns TOTP
    // on through the public API, then signs in with a wrong code (back to the password), the
    // app's code, and a recovery code.
    test("sign in with a password and a TOTP code or a recovery code (04 B15, B20, B21)", async ({
      browser,
    }) => {
      const context = await browser.newContext();
      const learner = await context.newPage();
      try {
        await signInWithPassword(learner);
        await expect(learner.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
        const enrolled = await learner.evaluate(async () => {
          const started = await fetch("/api/v1/me/totp", { method: "POST" });
          return { status: started.status, ...(await started.json()) };
        });
        expect(enrolled.status).toBe(200);
        const secret = base32(enrolled.secret);
        const confirmed = await learner.evaluate(
          async (code) => {
            const answer = await fetch("/api/v1/me/totp/confirm", {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ code }),
            });
            return { status: answer.status, ...(await answer.json()) };
          },
          totp(secret, 0),
        );
        expect(confirmed.status).toBe(200);
        expect(confirmed.recovery_codes).toHaveLength(10);
        const [recoveryCode] = confirmed.recovery_codes as string[];
        await context.clearCookies();

        await test.step("a wrong code needs the password again", async () => {
          await signInWithPassword(learner);
          await expect(
            learner.getByRole("heading", { level: 1, name: "Enter your code" }),
          ).toBeVisible();
          const code = learner.getByRole("textbox", { name: /^Code/ });
          await expect(code).toHaveAttribute("autocomplete", "one-time-code");
          const refused = answerOf(learner, "/auth/local/totp");
          // Six digits that are the code of no step the server accepts now.
          await code.fill(wrongCode(secret));
          await learner.getByRole("button", { name: "Sign in" }).click();
          expect((await refused).status()).toBe(401);
          await expect(
            learner.getByRole("heading", { level: 1, name: "Enter your password" }),
          ).toBeVisible();
          await expect(learner.getByRole("alert")).toContainText("That code did not work.");
          await expect(learner.locator('[data-slot="password-again"]')).toBeFocused();
        });

        await test.step("the app's code signs in", async () => {
          await learner.getByLabel("Password").fill(PHRASE);
          await learner.getByRole("button", { name: "Sign in" }).click();
          // The next step's code: the confirmation used the current one, and a code is used once.
          await learner.getByRole("textbox", { name: /^Code/ }).fill(totp(secret, 1));
          const accepted = answerOf(learner, "/auth/local/totp");
          await learner.getByRole("button", { name: "Sign in" }).click();
          expect((await accepted).status()).toBe(200);
          await expect(learner.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
        });

        await test.step("a recovery code signs in instead", async () => {
          await context.clearCookies();
          await signInWithPassword(learner);
          await learner.getByRole("button", { name: "Use a recovery code instead" }).click();
          const field = learner.getByRole("textbox", { name: /^Recovery code/ });
          await expect(field).toBeFocused();
          await field.fill(recoveryCode ?? "");
          const accepted = answerOf(learner, "/auth/local/totp");
          await learner.getByRole("button", { name: "Sign in" }).click();
          expect((await accepted).status()).toBe(200);
          await expect(learner.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
        });
      } finally {
        await context.close();
      }
    });

    // TOTP required by the organization (04 B15, Q11 (a)) end to end: the admin adds a user with a
    // password and turns the requirement on through the public API; the user's password sign-in
    // sets TOTP up on the screen (a wrong code first, then the app's), sees the recovery codes once
    // and lands signed in. The admin then removes the user's TOTP, and turns the requirement off.
    test("set up a required TOTP during sign-in (04 B15, Q11)", async ({ page, browser }) => {
      expect(passkey, "an earlier test left the admin a passkey").toBeDefined();
      const authenticator = await addVirtualAuthenticator(page, passkey);
      await page.goto("/sign-in");
      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      [passkey] = await authenticator.credentials();

      const casey = await page.evaluate(async (email) => {
        const created = await fetch("/api/v1/users", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ display_name: "Casey Example", email }),
        });
        const user = await created.json();
        const issued = await fetch(`/api/v1/admin/users/${user.id}/enrollment-links`, {
          method: "POST",
        });
        return { id: user.id as string, status: issued.status, ...(await issued.json()) };
      }, SET_UP_USER);
      expect(casey.status).toBe(201);
      /** Turns the requirement on or off as the admin, keeping every other switch. */
      const requireTotp = (on: boolean) =>
        page.evaluate(async (on) => {
          const current = await (await fetch("/api/v1/admin/local-sign-in")).json();
          const answer = await fetch("/api/v1/admin/local-sign-in", {
            method: "PUT",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ ...current, totp_required: on }),
          });
          return { status: answer.status, ...(await answer.json()) };
        }, on);

      const context = await browser.newContext();
      const learner = await context.newPage();
      try {
        await learner.goto("/sign-in");
        const token = new URL(casey.url).hash.replace(/^#token=/, "");
        const enrolled = await learner.evaluate(
          async ({ token, password }) =>
            (
              await fetch("/auth/local/enrollment/password", {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: JSON.stringify({ token, password }),
              })
            ).status,
          { token, password: PHRASE },
        );
        expect(enrolled).toBe(200);
        await context.clearCookies();
        const required = await requireTotp(true);
        expect([required.status, required.totp_required]).toEqual([200, true]);

        await learner.goto("/sign-in?return_to=%2Faccount");
        await learner.getByRole("textbox", { name: "Email" }).fill(SET_UP_USER);
        await learner.getByRole("button", { name: "Continue" }).click();
        await learner.getByRole("link", { name: "Sign in with a password instead" }).click();
        await learner.getByLabel("Password").fill(PHRASE);
        const started = answerOf(learner, "/auth/local/password");
        await learner.getByRole("button", { name: "Sign in" }).click();
        expect((await started).status()).toBe(200);
        await expect(
          learner.getByRole("heading", { level: 1, name: "Set up a code from an app" }),
        ).toBeVisible();
        expect((await learner.request.get("/api/v1/me")).status()).toBe(401);
        expect(learner.url()).not.toContain("secret");
        const text = await learner.locator('[data-slot="totp-setup-key"] code').textContent();
        const secret = base32(text ?? "");
        expect(await violations(learner)).toEqual([]);

        const refused = answerOf(learner, "/auth/local/totp/enrollment");
        await learner.getByRole("textbox", { name: /^Code/ }).fill(wrongCode(secret));
        await learner.getByRole("button", { name: "Turn on and sign in" }).click();
        expect((await refused).status()).toBe(401);
        await expect(learner.getByRole("alert")).toContainText("That code did not work.");

        const accepted = answerOf(learner, "/auth/local/totp/enrollment");
        await learner.getByRole("textbox", { name: /^Code/ }).fill(totp(secret, 0));
        await learner.getByRole("button", { name: "Turn on and sign in" }).click();
        expect((await accepted).status()).toBe(200);
        await expect(
          learner.getByRole("heading", { level: 1, name: "Save your recovery codes" }),
        ).toBeFocused();
        await expect(learner.getByRole("listitem")).toHaveCount(10);
        expect(await violations(learner)).toEqual([]);
        await learner.getByRole("button", { name: "Continue" }).click();
        await expect(learner.getByRole("heading", { level: 1, name: "Account" })).toBeVisible();

        const removed = await page.evaluate(
          async (id) =>
            (await fetch(`/api/v1/admin/users/${id}/totp`, { method: "DELETE" })).status,
          casey.id,
        );
        expect(removed).toBe(204);
      } finally {
        await context.close();
        // Back to the organization's default, so no later test meets the requirement.
        const restored = await requireTotp(false);
        expect([restored.status, restored.totp_required]).toEqual([200, false]);
      }
    });

    // Sign-in through the mock OpenID Connect server the stack runs (06 B3, T19; 11 B12, T16). The
    // organization creates no accounts at sign-in by default, so the admin turns that on through
    // the public API first (07 B12); the mock then signs in as its one user, "Test Student", who
    // gets an account on the way in.
    test("sign in through the mock OpenID Connect provider and sign out (06 T19, 11 T16)", async ({
      page,
      browser,
    }) => {
      expect(passkey, "an earlier test left the admin a passkey").toBeDefined();
      const authenticator = await addVirtualAuthenticator(page, passkey);
      await page.goto("/sign-in");
      await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
      await page.getByRole("button", { name: "Continue" }).click();
      await page.getByRole("button", { name: "Sign in with a passkey" }).click();
      await expect(page.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
      [passkey] = await authenticator.credentials();
      /** Turns account creation at sign-in on or off as the admin, through the public API. */
      const signupAllowed = (allowed: boolean) =>
        page.evaluate(async (allowed) => {
          const answer = await fetch("/api/v1/admin/admission", {
            method: "PUT",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ signup_allowed: allowed }),
          });
          return { status: answer.status, ...(await answer.json()) };
        }, allowed);
      const signup = await signupAllowed(true);
      expect([signup.status, signup.signup_allowed]).toEqual([200, true]);

      const context = await browser.newContext();
      const learner = await context.newPage();
      try {
        await learner.goto("/sign-in?return_to=%2Faccount");
        await expect(learner.getByRole("textbox", { name: "Email" })).toBeVisible();
        expect(await violations(learner)).toEqual([]);
        await learner.getByRole("link", { name: "Sign in with Mock ID" }).click();

        // Back from the mock signed in, on the return target, with the real `/me` loaded.
        await expect(learner.getByRole("heading", { level: 1, name: "Account" })).toBeVisible();
        expect(new URL(learner.url()).pathname).toBe("/account");
        const menu = learner.getByRole("button", { name: "Test Student, account menu" });
        await expect(menu).toBeVisible();
        const me = await learner.request.get("/api/v1/me");
        expect(me.status()).toBe(200);
        const user = (await me.json()).user;
        expect([user.display_name, user.email, user.source]).toEqual([
          "Test Student",
          "test.student@example.test",
          "idp",
        ]);
        expect(await violations(learner)).toEqual([]);

        await menu.click();
        await learner.getByRole("menuitem", { name: "Sign out" }).click();
        await expect(learner.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
        expect((await learner.request.get("/api/v1/me")).status()).toBe(401);

        // The second sign-in finds the account the first one made.
        await learner.getByRole("link", { name: "Sign in with Mock ID" }).click();
        await expect(learner.getByRole("heading", { level: 1, name: "Home" })).toBeVisible();
        await expect(menu).toBeVisible();
        const again = await learner.request.get("/api/v1/me");
        expect((await again.json()).user.id).toBe(user.id);
      } finally {
        await context.close();
        // Back to the organization's default, so no later test meets sign-up turned on.
        const restored = await signupAllowed(false);
        expect([restored.status, restored.signup_allowed]).toEqual([200, false]);
      }
    });
  });

/** Signs the password test's user in from the sign-in screen as far as the password takes them. */
async function signInWithPassword(page: Page) {
  await page.goto("/sign-in");
  await page.getByRole("textbox", { name: "Email" }).fill(PASSWORD_USER);
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("link", { name: "Sign in with a password instead" }).click();
  await page.getByLabel("Password").fill(PHRASE);
  await page.getByRole("button", { name: "Sign in" }).click();
}

/** The bytes of a base32 secret (RFC 4648, no padding), as an authenticator app reads it. */
function base32(text: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = "";
  for (const char of text.replace(/=+$/, "").toUpperCase()) {
    const value = alphabet.indexOf(char);
    if (value === -1) throw new Error("the secret is not base32");
    bits += value.toString(2).padStart(5, "0");
  }
  const bytes = bits.match(/.{8}/g) ?? [];
  return Buffer.from(bytes.map((byte) => Number.parseInt(byte, 2)));
}

/** Six digits that are not `secret`'s code for the step before, at or after now. */
function wrongCode(secret: Buffer): string {
  const near = new Set([-1, 0, 1, 2].map((offset) => totp(secret, offset)));
  let code = 0;
  while (near.has(String(code).padStart(6, "0"))) code++;
  return String(code).padStart(6, "0");
}

/**
 * The 6-digit code an authenticator app shows for `secret` (RFC 6238: HMAC-SHA-1, 30 s steps),
 * `offset` steps from now.
 */
function totp(secret: Buffer, offset: number): string {
  const step = Math.floor(Date.now() / 1000 / 30) + offset;
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const mac = createHmac("sha1", secret).update(counter).digest();
  const at = (mac[mac.length - 1] ?? 0) & 0x0f;
  const value = mac.readUInt32BE(at) & 0x7fffffff;
  return String(value % 1_000_000).padStart(6, "0");
}

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
