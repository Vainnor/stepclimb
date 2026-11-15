// SPDX-License-Identifier: AGPL-3.0-or-later
// The sign-in steps in jsdom (docs/specs/m0/11-app-shell.md B15 to B17, T7, T8): the email step,
// the passkey step against a stubbed server and WebAuthn, and the message for each problem code.
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  apiRequests,
  type Handler,
  json,
  meFixture,
  problem,
  renderApp,
  stubMatchMedia,
  stubServer,
} from "../test/app";
import {
  credentialJson,
  domError,
  removeWebAuthn,
  stubSecureContext,
  stubWebAuthn,
} from "../test/webauthn";

const requestOptions = {
  publicKey: {
    challenge: "Y2g",
    rpId: "localhost",
    allowCredentials: [],
    userVerification: "required",
  },
};

const started = () => json({ options: requestOptions });
const signedIn =
  (returnTo = "/") =>
  () =>
    json({ return_to: returnTo });
const me = () => json(meFixture());

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.unstubAllGlobals();
  removeWebAuthn();
});

async function enterEmail(email = "avery@example.test") {
  const user = userEvent.setup();
  await user.type(await screen.findByRole("textbox", { name: "Email" }), email);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByRole("heading", { level: 1, name: "Use your passkey" });
  return user;
}

async function pressSignIn(user: ReturnType<typeof userEvent.setup>) {
  const button = screen.getByRole("button", { name: "Sign in with a passkey" });
  await waitFor(() => expect(button).not.toHaveAttribute("data-pending"));
  await user.click(button);
}

async function bodyOf(fetch: ReturnType<typeof stubServer>, path: string): Promise<unknown> {
  const call = fetch.mock.calls.find(([r]) => new URL((r as Request).url).pathname === path);
  if (!call) throw new Error(`no request to ${path}`);
  return (call[0] as Request).clone().json();
}

function requestsTo(fetch: ReturnType<typeof stubServer>, path: string): Request[] {
  return fetch.mock.calls
    .map(([r]) => r as Request)
    .filter((r) => new URL(r.url).pathname === path);
}

describe("the email step", () => {
  it("hands the address to the passkey step in history state, never in the URL (B16)", async () => {
    stubWebAuthn();
    const fetch = stubServer({ "POST /auth/local/passkey/start": started });
    const app = renderApp("/sign-in?return_to=%2Fcourses%3Fx%3D1");
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    await enterEmail();

    const location = app.router.state.location;
    expect(location.pathname).toBe("/sign-in/passkey");
    expect(location.href).not.toContain("avery");
    expect(location.state.email).toBe("avery@example.test");
    expect(screen.getByText("Signing in as avery@example.test.")).toBeInTheDocument();
    await waitFor(() => expect(requestsTo(fetch, "/auth/local/passkey/start")).toHaveLength(1));
    expect(await bodyOf(fetch, "/auth/local/passkey/start")).toEqual({
      email: "avery@example.test",
      return_to: "/courses?x=1",
    });
  });

  it("sends no return target the client refuses (B15)", async () => {
    stubWebAuthn();
    const fetch = stubServer({ "POST /auth/local/passkey/start": started });
    renderApp("/sign-in?return_to=%2Fa%2F..%2F%2Fevil.example");
    await enterEmail();
    await waitFor(() => expect(requestsTo(fetch, "/auth/local/passkey/start")).toHaveLength(1));
    expect(await bodyOf(fetch, "/auth/local/passkey/start")).toEqual({
      email: "avery@example.test",
      return_to: "/",
    });
  });

  it("keeps the address in its own entry, so Back finds it filled in", async () => {
    stubWebAuthn();
    stubServer({ "POST /auth/local/passkey/start": started });
    const app = renderApp("/sign-in");
    await enterEmail();
    app.router.history.back();
    expect(await screen.findByRole("textbox", { name: "Email" })).toHaveValue("avery@example.test");
  });

  it("asks for an address before it moves on", async () => {
    const fetch = stubServer({});
    const user = userEvent.setup();
    renderApp("/sign-in");
    await user.click(await screen.findByRole("button", { name: "Continue" }));
    expect(screen.getByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(apiRequests(fetch)).toEqual([]);
  });
});

describe("the passkey step", () => {
  it("goes back to the email step when opened without an address", async () => {
    const fetch = stubServer({});
    const app = renderApp("/sign-in/passkey?return_to=%2Fcourses");
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/sign-in");
    expect(app.router.state.location.search).toEqual({ return_to: "/courses" });
    expect(apiRequests(fetch)).toEqual([]);
  });

  it("signs in with the passkey and opens the flow's return target", async () => {
    const webauthn = stubWebAuthn();
    const fetch = stubServer({
      "POST /auth/local/passkey/start": started,
      "POST /auth/local/passkey/finish": signedIn("/"),
      "GET /api/v1/me": me,
    });
    const app = renderApp("/sign-in");
    const user = await enterEmail();
    await pressSignIn(user);

    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(webauthn.get).toHaveBeenCalledWith({ publicKey: { parsed: requestOptions.publicKey } });
    expect(await bodyOf(fetch, "/auth/local/passkey/finish")).toEqual({
      credential: credentialJson,
    });
    expect(app.router.state.location.pathname).toBe("/");
  });

  it.each(["//evil.example", "https://evil.example/x", "/a/..//evil.example"])(
    "opens / when the server answers the return target %j (B15)",
    async (target) => {
      stubWebAuthn();
      stubServer({
        "POST /auth/local/passkey/start": started,
        "POST /auth/local/passkey/finish": signedIn(target),
        "GET /api/v1/me": me,
      });
      const app = renderApp("/sign-in");
      const user = await enterEmail();
      await pressSignIn(user);
      await screen.findByRole("heading", { level: 1, name: "Home" });
      expect(app.router.state.location.href).toBe("/");
    },
  );

  it("opens a kept return target with its search", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/passkey/start": started,
      "POST /auth/local/passkey/finish": signedIn("/?tab=2"),
      "GET /api/v1/me": me,
    });
    const app = renderApp("/sign-in?return_to=%2F%3Ftab%3D2");
    const user = await enterEmail();
    await pressSignIn(user);
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(app.router.state.location.href).toBe("/?tab=2");
  });

  it("loads a return target the server answers, such as an app's authorization, as a new page", async () => {
    stubWebAuthn();
    const target = "/oauth/authorize?client_id=stepclimb-app&state=s1";
    stubServer({
      "POST /auth/local/passkey/start": started,
      "POST /auth/local/passkey/finish": signedIn(target),
    });
    const app = renderApp("/sign-in");
    const user = await enterEmail();
    // jsdom cannot load a page: the spy stands in for the document navigation.
    const navigate = vi.spyOn(app.router, "navigate").mockResolvedValue(undefined);
    await pressSignIn(user);
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ href: target, replace: true, reloadDocument: true }),
    );
  });

  it("opens a web app route in place, not as a new page", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/passkey/start": started,
      "POST /auth/local/passkey/finish": signedIn("/oauthx"),
      "GET /api/v1/me": me,
    });
    const app = renderApp("/sign-in");
    const user = await enterEmail();
    const navigate = vi.spyOn(app.router, "navigate");
    await pressSignIn(user);
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({
        href: "/oauthx",
        replace: true,
        reloadDocument: false,
      }),
    );
  });

  it("forgets what it cached while signed out", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/passkey/start": started,
      "POST /auth/local/passkey/finish": signedIn(),
      "GET /api/v1/me": me,
    });
    const app = renderApp("/sign-in");
    app.context.queryClient.setQueryData(["me"], { stale: true });
    const user = await enterEmail();
    await pressSignIn(user);
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(app.context.queryClient.getQueryData(["me"])).toEqual(meFixture());
  });

  it("says when the browser cannot use passkeys, and starts no flow", async () => {
    stubSecureContext(true);
    const fetch = stubServer({});
    renderApp("/sign-in");
    await enterEmail();
    expect(screen.getByRole("alert")).toHaveTextContent("This browser cannot use passkeys.");
    expect(screen.queryByRole("button", { name: "Sign in with a passkey" })).toBeNull();
    expect(apiRequests(fetch)).toEqual([]);
  });

  it("says to open the https address on a page that is not a secure context", async () => {
    stubWebAuthn();
    stubSecureContext(false);
    const fetch = stubServer({});
    renderApp("/sign-in");
    await enterEmail();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Passkeys work only over a secure connection. Open this page at its https:// address",
    );
    expect(screen.queryByRole("button", { name: "Sign in with a passkey" })).toBeNull();
    expect(apiRequests(fetch)).toEqual([]);
  });

  it("counts a browser without credentials.get as one that cannot use passkeys", async () => {
    stubWebAuthn();
    Object.defineProperty(navigator, "credentials", {
      value: { create: async () => null },
      configurable: true,
    });
    stubServer({});
    renderApp("/sign-in");
    await enterEmail();
    expect(screen.getByRole("alert")).toHaveTextContent("This browser cannot use passkeys.");
  });

  it("says a cancelled prompt was cancelled, and sends nothing", async () => {
    const webauthn = stubWebAuthn();
    webauthn.get.mockRejectedValueOnce(domError("NotAllowedError"));
    const fetch = stubServer({ "POST /auth/local/passkey/start": started });
    renderApp("/sign-in");
    const user = await enterEmail();
    await pressSignIn(user);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The passkey request was cancelled or timed out.",
    );
    expect(requestsTo(fetch, "/auth/local/passkey/finish")).toHaveLength(0);
    expect(requestsTo(fetch, "/auth/local/passkey/start")).toHaveLength(1);
  });

  it("says what a browser failure means", async () => {
    const webauthn = stubWebAuthn();
    webauthn.get.mockRejectedValueOnce(domError("SecurityError"));
    stubServer({ "POST /auth/local/passkey/start": started });
    renderApp("/sign-in");
    const user = await enterEmail();
    await pressSignIn(user);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This browser could not use a passkey.",
    );
  });
});

describe("problem codes on the passkey step (B17, T8)", () => {
  const finishFails = (status: number, code: string): Record<string, Handler> => ({
    "POST /auth/local/passkey/start": started,
    "POST /auth/local/passkey/finish": () => problem(status, code, "Refused"),
  });

  it.each([
    [400, "auth.flow_expired", "The passkey request expired. Try again."],
    [
      400,
      "auth.flow_invalid",
      "This sign-in was already finished or replaced by one in another tab.",
    ],
    [401, "auth.passkey_invalid", "Stepclimb did not accept that passkey."],
  ])("%i %s says so and starts a new ceremony", async (status, code, message) => {
    stubWebAuthn();
    const fetch = stubServer(finishFails(status, code));
    renderApp("/sign-in");
    const user = await enterEmail();
    await pressSignIn(user);
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    await waitFor(() => expect(requestsTo(fetch, "/auth/local/passkey/start")).toHaveLength(2));
  });

  it.each([
    [403, "auth.suspended", "Your account is suspended. Ask an administrator to restore it."],
    [
      403,
      "auth.admission_refused",
      "Your organization's sign-in rules do not let this account in. Ask an administrator.",
    ],
    [
      403,
      "local.passkeys_not_allowed",
      "Your organization does not let you sign in with a passkey. Sign in with your identity provider instead.",
    ],
    [404, "auth.method_disabled", "Your organization has turned off passkey sign-in."],
    [503, "org.none", "This server has no organization yet."],
  ])("%i %s says so and keeps the flow", async (status, code, message) => {
    stubWebAuthn();
    const fetch = stubServer(finishFails(status, code));
    renderApp("/sign-in");
    const user = await enterEmail();
    await pressSignIn(user);
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(requestsTo(fetch, "/auth/local/passkey/start")).toHaveLength(1);
  });

  it("names the wait a rate limit gives", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/passkey/start": () => {
        const response = problem(429, "auth.rate_limited", "Too Many Requests");
        response.headers.set("retry-after", "42");
        return response;
      },
    });
    renderApp("/sign-in");
    await enterEmail();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Too many sign-in attempts from your network. Try again in 42 seconds.",
    );
  });

  it("says to wait when a rate limit gives no time", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/passkey/start": () => problem(429, "auth.rate_limited", "Busy"),
    });
    renderApp("/sign-in");
    await enterEmail();
    expect(await screen.findByRole("alert")).toHaveTextContent("Wait a minute, then try again.");
  });

  it("starts again when the button is pressed after the start failed", async () => {
    stubWebAuthn();
    let calls = 0;
    const fetch = stubServer({
      "POST /auth/local/passkey/start": () =>
        ++calls === 1 ? problem(429, "auth.rate_limited", "Busy") : started(),
    });
    renderApp("/sign-in");
    const user = await enterEmail();
    await screen.findByRole("alert");
    await pressSignIn(user);
    await waitFor(() => expect(requestsTo(fetch, "/auth/local/passkey/start")).toHaveLength(2));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("shows an unknown code in mono with the general message", async () => {
    stubWebAuthn();
    stubServer(finishFails(418, "auth.teapot"));
    renderApp("/sign-in");
    const user = await enterEmail();
    await pressSignIn(user);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong while signing you in.");
    expect(alert.querySelector("code")).toHaveTextContent("auth.teapot");
  });

  it("says the server could not be reached when nothing answered", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/passkey/start": () => Promise.reject(new TypeError("Failed to fetch")),
    });
    renderApp("/sign-in");
    await enterEmail();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Stepclimb could not reach the server.",
    );
  });
});
