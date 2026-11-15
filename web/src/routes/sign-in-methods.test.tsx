// SPDX-License-Identifier: AGPL-3.0-or-later
// The method list, the callback error and the password step in jsdom (docs/specs/m0/11-app-shell.md
// B13 to B18, T6, T8), against a stubbed server: what the discovery document offers is all the
// screen draws, a provider button leaves the app for the provider's start route, a code a browser
// flow came back with is shown once and removed from the address, and the password step signs in.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { openDocument } from "../document-navigation";
import type { SignInMethod } from "../sign-in/discovery";
import {
  apiRequests,
  discoveryFixture,
  json,
  meFixture,
  problem,
  renderApp,
  requestsTo,
  stubMatchMedia,
  stubServer,
} from "../test/app";
import { removeWebAuthn, stubWebAuthn } from "../test/webauthn";

vi.mock("../document-navigation", () => ({ openDocument: vi.fn() }));

const vatsim: SignInMethod = { key: "vatsim", kind: "oauth2", label: "Sign in with VATSIM" };
const okta: SignInMethod = { key: "okta", kind: "oidc", label: "Sign in with Example ID" };
const discovery =
  (...args: Parameters<typeof discoveryFixture>) =>
  () =>
    json(discoveryFixture(...args));
const me = () => json(meFixture());

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.mocked(openDocument).mockClear();
  removeWebAuthn();
});

async function bodyOf(fetch: ReturnType<typeof stubServer>, path: string): Promise<unknown> {
  const [request] = requestsTo(fetch, path);
  if (!request) throw new Error(`no request to ${path}`);
  return request.clone().json();
}

describe("the method list (B13, B14, T6)", () => {
  it("shows two provider buttons with their labels and no email field without local sign-in", async () => {
    stubServer({ "GET /.well-known/stepclimb": discovery(null, [okta, vatsim]) });
    renderApp("/sign-in?return_to=%2Fcourses%3Fx%3D1");
    const first = await screen.findByRole("link", { name: "Sign in with Example ID" });
    const second = screen.getByRole("link", { name: "Sign in with VATSIM" });
    expect(screen.queryByRole("textbox", { name: "Email" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Continue" })).toBeNull();
    expect(first).toHaveAttribute("href", "/auth/okta/start?return_to=%2Fcourses%3Fx%3D1");
    expect(second).toHaveAttribute("href", "/auth/vatsim/start?return_to=%2Fcourses%3Fx%3D1");
    // Two ways in: neither is the view's primary action.
    expect(first).not.toHaveClass("bg-brand");
    expect(second).not.toHaveClass("bg-brand");
  });

  it("lists the local email step first, then the providers, in the document's order", async () => {
    stubServer({
      "GET /.well-known/stepclimb": discovery(["passkey", "password"], [okta, vatsim]),
    });
    renderApp("/sign-in");
    const email = await screen.findByRole("textbox", { name: "Email" });
    const links = screen.getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual([
      "Sign in with Example ID",
      "Sign in with VATSIM",
    ]);
    // Document order: the field comes before the first provider.
    expect(
      email.compareDocumentPosition(links[0] as Node) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByRole("separator")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toHaveClass("bg-brand");
    expect(links[0]).toHaveAttribute("href", "/auth/okta/start");
  });

  it("makes a provider the primary action when it is the only way in", async () => {
    stubServer({ "GET /.well-known/stepclimb": discovery(null, [vatsim]) });
    renderApp("/sign-in");
    expect(await screen.findByRole("link", { name: "Sign in with VATSIM" })).toHaveClass(
      "bg-brand",
    );
  });

  it("names a provider without a label by its key", async () => {
    stubServer({
      "GET /.well-known/stepclimb": discovery(null, [
        { key: "corp-sso", kind: "oidc", label: null },
      ]),
    });
    renderApp("/sign-in");
    expect(await screen.findByRole("link", { name: "Sign in with corp-sso" })).toHaveAttribute(
      "href",
      "/auth/corp-sso/start",
    );
  });

  it("leaves out a provider whose key is . or .., which would not stay one path segment", async () => {
    stubServer({
      "GET /.well-known/stepclimb": discovery(null, [
        { key: ".", kind: "oidc", label: "Dot" },
        { key: "..", kind: "oidc", label: "Dot dot" },
        vatsim,
      ]),
    });
    renderApp("/sign-in");
    await screen.findByRole("link", { name: "Sign in with VATSIM" });
    expect(screen.queryByRole("link", { name: "Dot" })).toBeNull();
    expect(screen.queryByRole("link", { name: "Dot dot" })).toBeNull();
    expect(screen.getAllByRole("link")).toHaveLength(1);
  });

  it("percent-encodes / and ? in a key, so it stays one path segment", async () => {
    stubServer({
      "GET /.well-known/stepclimb": discovery(null, [{ key: "a/b?c", kind: "oidc", label: "Odd" }]),
    });
    renderApp("/sign-in");
    expect(await screen.findByRole("link", { name: "Odd" })).toHaveAttribute(
      "href",
      "/auth/a%2Fb%3Fc/start",
    );
  });

  it("opens the start route as a full page, not a client-side navigation (B14)", async () => {
    const fetch = stubServer({ "GET /.well-known/stepclimb": discovery(null, [okta, vatsim]) });
    const app = renderApp("/sign-in?return_to=%2Fcourses");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("link", { name: "Sign in with VATSIM" }));
    expect(openDocument).toHaveBeenCalledExactlyOnceWith("/auth/vatsim/start?return_to=%2Fcourses");
    expect(app.router.state.location.pathname).toBe("/sign-in");
    // Not a fetch: the page asked the server for nothing but the document.
    expect(apiRequests(fetch)).toEqual([]);
  });

  it("draws nothing for TOTP or the magic link on their own", async () => {
    stubServer({ "GET /.well-known/stepclimb": discovery(["totp", "magic_link"]) });
    renderApp("/sign-in");
    expect(
      await screen.findByText(
        "This organization has not turned on a way to sign in from the browser yet. Ask an administrator how to sign in.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("says so when the organization offers no way to sign in", async () => {
    stubServer({ "GET /.well-known/stepclimb": discovery(null, []) });
    renderApp("/sign-in");
    expect(
      await screen.findByText(/has not turned on a way to sign in from the browser yet/),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByRole("link")).toBeNull();
  });

  it("names the organization in the header and the document title", async () => {
    stubServer({});
    renderApp("/sign-in");
    await screen.findByRole("textbox", { name: "Email" });
    expect(within(screen.getByRole("banner")).getByText("Example Training")).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Sign in · Example Training"));
  });

  it("shows a skeleton while the document loads", async () => {
    let answer: (response: Response) => void = () => undefined;
    stubServer({
      "GET /.well-known/stepclimb": () =>
        new Promise<Response>((resolve) => {
          answer = resolve;
        }),
    });
    renderApp("/sign-in");
    await screen.findByRole("heading", { level: 1, name: "Sign in" });
    expect(document.querySelector('main [aria-busy="true"]')).not.toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    answer(json(discoveryFixture()));
    expect(await screen.findByRole("textbox", { name: "Email" })).toBeInTheDocument();
    expect(document.querySelector('main [aria-busy="true"]')).toBeNull();
  });

  it("shows the failure with Try again when the document does not load, and the product name", async () => {
    let calls = 0;
    stubServer({
      "GET /.well-known/stepclimb": () =>
        ++calls === 1 ? problem(503, "org.none", "No organization") : json(discoveryFixture()),
    });
    renderApp("/sign-in");
    expect(
      await screen.findByRole("heading", { level: 1, name: "This server has no organization yet" }),
    ).toBeInTheDocument();
    expect(within(screen.getByRole("banner")).getByText("Stepclimb")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("textbox", { name: "Email" })).toBeInTheDocument();
    expect(calls).toBe(2);
  });
});

describe("the code a browser flow came back with (B18, T8)", () => {
  it.each([
    ["auth.rate_limited", "Too many sign-in attempts from your network. Wait a minute"],
    ["auth.provider_not_found", "That way to sign in is not available."],
    ["auth.provider_unavailable", "Stepclimb could not start the sign-in with that service."],
    ["auth.provider_denied", "The sign-in was declined at the sign-in service"],
    ["auth.provider_error", "The sign-in service reported a problem"],
    ["auth.id_token_invalid", "Stepclimb could not verify the answer from the sign-in service"],
    ["auth.subject_missing", "The sign-in service did not say who you are"],
    ["auth.flow_expired", "The sign-in took too long to finish. Start it again."],
    ["auth.flow_invalid", "This sign-in was already finished or replaced by one in another tab."],
    ["auth.suspended", "Your account is suspended. Ask an administrator to restore it."],
    ["auth.signup_closed", "There is no account for you in this organization"],
    ["auth.not_provisioned", "This organization lets in only people it already has an account"],
    ["auth.admission_refused", "Your organization's sign-in rules do not let this account in."],
    ["entitlement.limit_reached", "This organization has no room for another user on its plan"],
  ])("%s has its own message", async (code, message) => {
    stubServer({ "GET /.well-known/stepclimb": discovery(null, [vatsim]) });
    renderApp(`/sign-in?error=${code}`);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(message);
    expect(alert.querySelector("code")).toBeNull();
  });

  it("removes the parameter from the address and keeps the message and the return target", async () => {
    stubServer({ "GET /.well-known/stepclimb": discovery(null, [vatsim]) });
    const app = renderApp("/sign-in?error=auth.provider_denied&return_to=%2Fcourses");
    await screen.findByRole("alert");
    await waitFor(() =>
      expect(app.router.state.location.search).toEqual({ return_to: "/courses" }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("The sign-in was declined");
    expect(screen.getByRole("link", { name: "Sign in with VATSIM" })).toHaveAttribute(
      "href",
      "/auth/vatsim/start?return_to=%2Fcourses",
    );
  });

  it("shows an unknown code in mono with the general message", async () => {
    stubServer({});
    renderApp("/sign-in?error=auth.something_new");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong while signing you in.");
    expect(alert.querySelector("code")).toHaveTextContent("auth.something_new");
  });

  it("moves focus to the message once, so it is read first", async () => {
    stubServer({ "GET /.well-known/stepclimb": discovery(null, [vatsim]) });
    renderApp("/sign-in?error=auth.provider_denied");
    const alert = await screen.findByRole("alert");
    await waitFor(() => expect(alert.closest('[data-slot="callback-error"]')).toHaveFocus());
    await screen.findByRole("link", { name: "Sign in with VATSIM" });
    expect(alert.closest('[data-slot="callback-error"]')).toHaveFocus();
  });

  it("leaves focus alone when the page came with no code", async () => {
    stubServer({});
    renderApp("/sign-in");
    await screen.findByRole("textbox", { name: "Email" });
    expect(document.querySelector('[data-slot="callback-error"]')).toBeNull();
    expect(document.activeElement).toBe(document.body);
  });

  it.each([
    "admission.new_rule",
    "entitlement.something_else",
    "local.new_check",
    "org.moved",
    "auth.flow.renamed",
  ])("shows %s, a code in a sign-in namespace, in mono", async (code) => {
    stubServer({});
    renderApp(`/sign-in?error=${code}`);
    expect((await screen.findByRole("alert")).querySelector("code")).toHaveTextContent(code);
  });

  it.each([
    "Call 555-0100 to restore your account",
    "call.support.now",
    "auth.a.b.c",
    "billing.overdue",
    "auth.x<script>",
    `auth.${"a".repeat(64)}`,
    "nodot",
  ])("never puts a value without a code's shape on the page: %j", async (value) => {
    stubServer({});
    renderApp(`/sign-in?error=${encodeURIComponent(value)}`);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong while signing you in. Try again.");
    expect(alert.querySelector("code")).toBeNull();
    expect(document.body.textContent).not.toContain(value);
  });

  it("shows nothing when the page came with no code", async () => {
    stubServer({});
    renderApp("/sign-in");
    await screen.findByRole("textbox", { name: "Email" });
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

const passwordOnly = { "GET /.well-known/stepclimb": discovery(["password"]) };

async function enterEmail(email = "avery@example.test") {
  const user = userEvent.setup();
  await user.type(await screen.findByRole("textbox", { name: "Email" }), email);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  return user;
}

async function enterPassword(user: ReturnType<typeof userEvent.setup>, password: string) {
  await screen.findByRole("heading", { level: 1, name: "Enter your password" });
  await user.type(screen.getByLabelText("Password"), password);
  await user.click(screen.getByRole("button", { name: "Sign in" }));
}

describe("the password step (B16, B17)", () => {
  it("follows the email step when passkeys are not offered, and signs in", async () => {
    const fetch = stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () => json({ return_to: "/?tab=2" }),
      "GET /api/v1/me": me,
    });
    const app = renderApp("/sign-in?return_to=%2F%3Ftab%3D2");
    const user = await enterEmail();
    expect(app.router.state.location.pathname).toBe("/sign-in/password");
    expect(screen.getByText("Signing in as avery@example.test.")).toBeInTheDocument();
    await enterPassword(user, "correct horse battery");

    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(app.router.state.location.href).toBe("/?tab=2");
    expect(await bodyOf(fetch, "/auth/local/password")).toEqual({
      email: "avery@example.test",
      password: "correct horse battery",
      return_to: "/?tab=2",
    });
    expect(requestsTo(fetch, "/auth/local/passkey/start")).toEqual([]);
  });

  it("names the account in a hidden, read-only username field for password managers", async () => {
    stubServer(passwordOnly);
    renderApp("/sign-in");
    await enterEmail();
    await screen.findByRole("heading", { level: 1, name: "Enter your password" });
    const username = document.querySelector<HTMLInputElement>('input[autocomplete="username"]');
    expect(username).not.toBeNull();
    expect(username?.form).toBe(screen.getByLabelText("Password").closest("form"));
    expect(username).toHaveValue("avery@example.test");
    expect(username).toHaveAttribute("readonly");
    expect(username).toHaveAttribute("hidden");
    expect(username).not.toBeVisible();
  });

  it("never puts the password in the URL or the history", async () => {
    let finish: (response: Response) => void = () => undefined;
    stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () =>
        new Promise<Response>((resolve) => {
          finish = resolve;
        }),
    });
    const app = renderApp("/sign-in");
    const user = await enterEmail();
    await enterPassword(user, "correct horse battery");
    const location = app.router.state.location;
    expect(location.href).not.toContain("horse");
    expect(JSON.stringify(location.state)).not.toContain("horse");
    finish(problem(401, "auth.credentials_invalid", "Unauthorized"));
    await screen.findByRole("alert");
  });

  it("sends no return target the client refuses (B15)", async () => {
    const fetch = stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () => json({ return_to: "//evil.example" }),
      "GET /api/v1/me": me,
    });
    const app = renderApp("/sign-in?return_to=%2F%2Fevil.example");
    const user = await enterEmail();
    await enterPassword(user, "correct horse battery");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(await bodyOf(fetch, "/auth/local/password")).toMatchObject({ return_to: "/" });
    expect(app.router.state.location.href).toBe("/");
  });

  it.each([
    [401, "auth.credentials_invalid", "That email and password do not match."],
    [403, "auth.suspended", "Your account is suspended."],
    [404, "auth.method_disabled", "Your organization has turned off password sign-in."],
    [
      403,
      "auth.admission_refused",
      "Your organization's sign-in rules do not let this account in.",
    ],
    [503, "org.none", "This server has no organization yet."],
  ])("%i %s says so", async (status, code, message) => {
    stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () => problem(status, code, "Refused"),
    });
    renderApp("/sign-in");
    const user = await enterEmail();
    await enterPassword(user, "wrong password here");
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(screen.getByRole("heading", { level: 1, name: "Enter your password" })).toBeVisible();
  });

  it("names the wait a rate limit gives, without blaming the network", async () => {
    stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () => {
        const response = problem(429, "auth.rate_limited", "Too Many Requests");
        response.headers.set("retry-after", "900");
        return response;
      },
    });
    renderApp("/sign-in");
    const user = await enterEmail();
    await enterPassword(user, "wrong password here");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Too many sign-in attempts. Try again in 900 seconds.");
    expect(alert).not.toHaveTextContent("network");
  });

  it("goes back to the email step when opened without an address", async () => {
    const fetch = stubServer(passwordOnly);
    const app = renderApp("/sign-in/password?return_to=%2Fcourses");
    expect(await screen.findByRole("textbox", { name: "Email" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/sign-in");
    expect(app.router.state.location.search).toEqual({ return_to: "/courses" });
    expect(apiRequests(fetch)).toEqual([]);
  });

  it("links to the passkey step with the address when passkeys are offered too", async () => {
    stubWebAuthn();
    stubServer({
      "GET /.well-known/stepclimb": discovery(["passkey", "password"]),
      "POST /auth/local/passkey/start": () =>
        json({ options: { publicKey: { challenge: "Y2g", rpId: "localhost" } } }),
    });
    const app = renderApp("/sign-in");
    const user = await enterEmail();
    await screen.findByRole("heading", { level: 1, name: "Use your passkey" });
    await user.click(screen.getByRole("link", { name: "Sign in with a password instead" }));
    await screen.findByRole("heading", { level: 1, name: "Enter your password" });
    expect(app.router.state.location.pathname).toBe("/sign-in/password");
    expect(app.router.state.location.state.email).toBe("avery@example.test");
    expect(screen.getByText("Signing in as avery@example.test.")).toBeInTheDocument();

    await user.click(screen.getByRole("link", { name: "Sign in with a passkey instead" }));
    await screen.findByRole("heading", { level: 1, name: "Use your passkey" });
    expect(app.router.state.location.state.email).toBe("avery@example.test");
  });

  it("offers no passkey link when passkeys are not offered, and the passkey step no password link", async () => {
    stubWebAuthn();
    stubServer({
      "GET /.well-known/stepclimb": discovery(["passkey"]),
      "POST /auth/local/passkey/start": () =>
        json({ options: { publicKey: { challenge: "Y2g", rpId: "localhost" } } }),
    });
    renderApp("/sign-in");
    await enterEmail();
    await screen.findByRole("heading", { level: 1, name: "Use your passkey" });
    expect(screen.queryByRole("link", { name: "Sign in with a password instead" })).toBeNull();
  });

  it("sends one request when Enter is pressed again while it is in flight", async () => {
    let finish: (response: Response) => void = () => undefined;
    const fetch = stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () =>
        new Promise<Response>((resolve) => {
          finish = resolve;
        }),
    });
    renderApp("/sign-in");
    const user = await enterEmail();
    await enterPassword(user, "correct horse battery");
    await user.type(screen.getByLabelText("Password"), "{Enter}");
    expect(requestsTo(fetch, "/auth/local/password")).toHaveLength(1);
    finish(problem(401, "auth.credentials_invalid", "Unauthorized"));
    await screen.findByRole("alert");
  });
});

describe("the TOTP step (04 B15, B20, B21; 11 B16, B17)", () => {
  const needsCode = { "POST /auth/local/password": () => json({ next: "totp" }) };

  async function reachTotp(path = "/sign-in") {
    const app = renderApp(path);
    const user = await enterEmail();
    await enterPassword(user, "correct horse battery");
    await screen.findByRole("heading", { level: 1, name: "Enter your code" });
    return { app, user };
  }

  async function enterCode(user: ReturnType<typeof userEvent.setup>, code: string) {
    await user.type(screen.getByRole("textbox", { name: /^Code/ }), code);
    await user.click(screen.getByRole("button", { name: "Sign in" }));
  }

  it("does not count {next: totp} as signed in, and asks for the app's code", async () => {
    const fetch = stubServer({ ...passwordOnly, ...needsCode });
    const { app } = await reachTotp("/sign-in?return_to=%2Fcourses");
    expect(app.router.state.location.pathname).toBe("/sign-in/totp");
    expect(app.router.state.location.search).toEqual({ return_to: "/courses" });
    expect(requestsTo(fetch, "/api/v1/me")).toEqual([]);
    expect(screen.getByText("Signing in as avery@example.test.")).toBeInTheDocument();
    const input = screen.getByRole("textbox", { name: /^Code/ });
    expect(input).toHaveAttribute("autocomplete", "one-time-code");
    expect(input).toHaveAttribute("inputmode", "numeric");
    expect(input).toHaveAccessibleDescription(
      "Open your authenticator app and enter the 6-digit code it shows for this account.",
    );
  });

  it("signs in with the code and opens the return target", async () => {
    const fetch = stubServer({
      ...passwordOnly,
      ...needsCode,
      "POST /auth/local/totp": () => json({ return_to: "/?tab=2" }),
      "GET /api/v1/me": me,
    });
    const { app, user } = await reachTotp("/sign-in?return_to=%2F%3Ftab%3D2");
    await enterCode(user, "123456");
    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(app.router.state.location.href).toBe("/?tab=2");
    expect(await bodyOf(fetch, "/auth/local/totp")).toEqual({ code: "123456" });
  });

  it("takes a recovery code instead, in a field of its own that takes focus", async () => {
    const fetch = stubServer({
      ...passwordOnly,
      ...needsCode,
      "POST /auth/local/totp": () => json({ return_to: "/" }),
      "GET /api/v1/me": me,
    });
    const { user } = await reachTotp();
    await user.type(screen.getByRole("textbox", { name: /^Code/ }), "12");
    await user.click(screen.getByRole("button", { name: "Use a recovery code instead" }));
    const recovery = screen.getByRole("textbox", { name: /^Recovery code/ });
    await waitFor(() => expect(recovery).toHaveFocus());
    expect(recovery).toHaveValue("");
    expect(recovery).toHaveAttribute("autocomplete", "off");
    expect(screen.queryByRole("textbox", { name: /^Code/ })).toBeNull();
    await user.type(recovery, "abcd-efgh-ijkl-mnop");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(await bodyOf(fetch, "/auth/local/totp")).toEqual({ code: "abcd-efgh-ijkl-mnop" });

    expect(screen.queryByRole("button", { name: "Use a code from your app instead" })).toBeNull();
  });

  it("switches back to the app's code", async () => {
    stubServer({ ...passwordOnly, ...needsCode });
    const { user } = await reachTotp();
    await user.click(screen.getByRole("button", { name: "Use a recovery code instead" }));
    await user.click(screen.getByRole("button", { name: "Use a code from your app instead" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: /^Code/ })).toHaveFocus());
  });

  it.each([
    ["auth.totp_invalid", 401, "That code did not work. Enter your password again"],
    ["auth.flow_expired", 400, "The sign-in took too long to finish. Enter your password again"],
    [
      "auth.flow_invalid",
      400,
      "This sign-in was already finished or replaced by one in another tab. Enter your password again.",
    ],
  ])(
    "after %s, goes back to the password step with the reason, focused",
    async (code, status, message) => {
      stubServer({
        ...passwordOnly,
        ...needsCode,
        "POST /auth/local/totp": () => problem(status, code, "Refused"),
      });
      const { app, user } = await reachTotp();
      await enterCode(user, "000000");
      await screen.findByRole("heading", { level: 1, name: "Enter your password" });
      const reason = screen.getByRole("alert");
      expect(reason).toHaveTextContent(message);
      await waitFor(() => expect(reason.closest('[data-slot="password-again"]')).toHaveFocus());
      expect(app.router.state.location.state.email).toBe("avery@example.test");
      expect(screen.getByLabelText("Password")).toHaveValue("");
      // The TOTP step's entry was replaced: Back goes to the earlier password entry, never to the
      // step of a used flow.
      const here = app.router.state.location.state.key;
      app.router.history.back();
      await waitFor(() => expect(app.router.state.location.state.key).not.toBe(here));
      expect(app.router.state.location.pathname).toBe("/sign-in/password");
      expect(app.router.state.location.state.passwordAgain).toBeUndefined();
    },
  );

  it("drops the carried reason once the password is sent again", async () => {
    let password = 0;
    stubServer({
      ...passwordOnly,
      "POST /auth/local/password": () =>
        ++password === 1
          ? json({ next: "totp" })
          : problem(401, "auth.credentials_invalid", "Unauthorized"),
      "POST /auth/local/totp": () => problem(401, "auth.totp_invalid", "Refused"),
    });
    const { user } = await reachTotp();
    await enterCode(user, "000000");
    await screen.findByRole("heading", { level: 1, name: "Enter your password" });
    await enterPassword(user, "wrong");
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("That email and password do not match."),
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
  });

  it.each([
    [403, "auth.suspended", "Your account is suspended."],
    [
      403,
      "auth.admission_refused",
      "Your organization's sign-in rules do not let this account in.",
    ],
    [
      404,
      "auth.method_disabled",
      "Your organization has turned off sign-in with a password and code.",
    ],
  ])("%i %s stays on the step and says so", async (status, code, message) => {
    const fetch = stubServer({
      ...passwordOnly,
      ...needsCode,
      "POST /auth/local/totp": () => problem(status, code, "Refused"),
    });
    const { app, user } = await reachTotp();
    await enterCode(user, "123456");
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(app.router.state.location.pathname).toBe("/sign-in/totp");
    expect(requestsTo(fetch, "/auth/local/password")).toHaveLength(1);
  });

  it("names the wait a rate limit gives, and stays", async () => {
    stubServer({
      ...passwordOnly,
      ...needsCode,
      "POST /auth/local/totp": () => {
        const response = problem(429, "auth.rate_limited", "Too Many Requests");
        response.headers.set("retry-after", "30");
        return response;
      },
    });
    const { app, user } = await reachTotp();
    await enterCode(user, "123456");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Too many sign-in attempts. Try again in 30 seconds.",
    );
    expect(app.router.state.location.pathname).toBe("/sign-in/totp");
  });

  it("sends one request when Enter is pressed again while it is in flight", async () => {
    let finish: (response: Response) => void = () => undefined;
    const fetch = stubServer({
      ...passwordOnly,
      ...needsCode,
      "POST /auth/local/totp": () =>
        new Promise<Response>((resolve) => {
          finish = resolve;
        }),
    });
    const { user } = await reachTotp();
    await enterCode(user, "123456");
    await user.type(screen.getByRole("textbox", { name: /^Code/ }), "{Enter}");
    expect(requestsTo(fetch, "/auth/local/totp")).toHaveLength(1);
    finish(problem(403, "auth.suspended", "Forbidden"));
    await screen.findByRole("alert");
  });

  it("goes back to the email step when opened without an address", async () => {
    const fetch = stubServer(passwordOnly);
    const app = renderApp("/sign-in/totp?return_to=%2Fcourses");
    expect(await screen.findByRole("textbox", { name: "Email" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/sign-in");
    expect(apiRequests(fetch)).toEqual([]);
  });
});

describe("TOTP set-up during sign-in (04 B15, Q11 (a); 11 B16, B17)", () => {
  const secret = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";
  const uri = `otpauth://totp/Example%20Training:avery%40example.test?secret=${secret}&issuer=Example%20Training&algorithm=SHA1&digits=6&period=30`;
  const needsSetUp = {
    "POST /auth/local/password": () => json({ next: "totp_enroll", otpauth_uri: uri, secret }),
  };
  const codes = Array.from(
    { length: 10 },
    (_, i) => `AAAA-BBBB-CCCC-DD${String(i).padStart(2, "0")}`,
  );

  async function reachSetUp(path = "/sign-in") {
    const app = renderApp(path);
    const user = await enterEmail();
    await enterPassword(user, "correct horse battery");
    await screen.findByRole("heading", { level: 1, name: "Set up a code from an app" });
    return { app, user };
  }

  async function enterCode(user: ReturnType<typeof userEvent.setup>, code: string) {
    await user.type(screen.getByRole("textbox", { name: /^Code/ }), code);
    await user.click(screen.getByRole("button", { name: "Turn on and sign in" }));
  }

  it("shows the setup key and link, keeps the secret out of the history, and signs no one in", async () => {
    const fetch = stubServer({ ...passwordOnly, ...needsSetUp });
    const { app } = await reachSetUp("/sign-in?return_to=%2Fcourses");
    expect(app.router.state.location.pathname).toBe("/sign-in/totp-setup");
    expect(app.router.state.location.search).toEqual({ return_to: "/courses" });
    expect(JSON.stringify(app.router.state.location.state)).not.toContain(secret);
    expect(app.router.state.location.href).not.toContain(secret);
    expect(requestsTo(fetch, "/api/v1/me")).toEqual([]);
    expect(screen.getByText(secret)).toBeInTheDocument();
    expect(screen.getByText(uri)).toBeInTheDocument();
    const input = screen.getByRole("textbox", { name: /^Code/ });
    expect(input).toHaveAttribute("autocomplete", "one-time-code");
    expect(input).toHaveAttribute("inputmode", "numeric");
  });

  it("stays after a wrong code, then shows the recovery codes once and opens the return target", async () => {
    let sent = 0;
    const fetch = stubServer({
      ...passwordOnly,
      ...needsSetUp,
      "POST /auth/local/totp/enrollment": () =>
        ++sent === 1
          ? problem(401, "auth.totp_invalid", "Refused")
          : json({ return_to: "/?tab=2", recovery_codes: codes }),
      "GET /api/v1/me": me,
    });
    const { app, user } = await reachSetUp("/sign-in?return_to=%2F%3Ftab%3D2");
    await enterCode(user, "000000");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "That code did not work. Check that your device's clock is right",
    );
    expect(app.router.state.location.pathname).toBe("/sign-in/totp-setup");

    const field = screen.getByRole("textbox", { name: /^Code/ });
    await user.clear(field);
    await enterCode(user, "123456");
    const heading = await screen.findByRole("heading", {
      level: 1,
      name: "Save your recovery codes",
    });
    await waitFor(() => expect(heading).toHaveFocus());
    const list = screen.getByRole("list");
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(codes);
    expect(await bodyOf(fetch, "/auth/local/totp/enrollment")).toEqual({ code: "000000" });
    expect(screen.queryByText(secret)).toBeNull();

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(app.router.state.location.href).toBe("/?tab=2");
  });

  it.each([
    ["auth.flow_expired", 400, "The sign-in took too long to finish. Enter your password again"],
    [
      "auth.flow_invalid",
      400,
      "This sign-in was already finished or replaced by one in another tab.",
    ],
  ])("after %s, goes back to the password step with the reason", async (code, status, message) => {
    stubServer({
      ...passwordOnly,
      ...needsSetUp,
      "POST /auth/local/totp/enrollment": () => problem(status, code, "Refused"),
    });
    const { app, user } = await reachSetUp();
    await enterCode(user, "123456");
    await screen.findByRole("heading", { level: 1, name: "Enter your password" });
    expect(screen.getByRole("alert")).toHaveTextContent(message);
    expect(app.router.state.location.state.email).toBe("avery@example.test");
  });

  it("goes back to the password step when the secret is gone (a reload)", async () => {
    const fetch = stubServer(passwordOnly);
    const app = renderApp("/sign-in/totp-setup", { email: "avery@example.test" });
    await screen.findByRole("heading", { level: 1, name: "Enter your password" });
    expect(app.router.state.location.pathname).toBe("/sign-in/password");
    expect(screen.getByRole("alert")).toHaveTextContent("Enter your password again.");
    expect(requestsTo(fetch, "/auth/local/totp/enrollment")).toEqual([]);
  });

  it("goes back to the email step when opened without an address", async () => {
    stubServer(passwordOnly);
    const app = renderApp("/sign-in/totp-setup");
    expect(await screen.findByRole("textbox", { name: "Email" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/sign-in");
  });
});
