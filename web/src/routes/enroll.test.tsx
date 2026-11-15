// SPDX-License-Identifier: AGPL-3.0-or-later
// The enrollment page in jsdom (docs/specs/m0/04-local-accounts.md B7, B8;
// 11-app-shell.md B17, T8): the token from the fragment, the passkey registration against a
// stubbed server and WebAuthn, and the message for each problem code.
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
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
import { enrollmentToken } from "./enroll";

const token = "test-token-of-the-enrollment-page";
const creationOptions = {
  publicKey: { challenge: "Y2g", rp: { id: "localhost", name: "Example" } },
};

const started =
  (methods: string[] = ["passkey", "password"]) =>
  () =>
    json({
      email: "avery@example.test",
      methods,
      ...(methods.includes("passkey") ? { passkey_options: creationOptions } : {}),
    });

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.unstubAllGlobals();
  removeWebAuthn();
});

function requestsTo(fetch: ReturnType<typeof stubServer>, path: string): Request[] {
  return fetch.mock.calls
    .map(([r]) => r as Request)
    .filter((r) => new URL(r.url).pathname === path);
}

async function create() {
  const user = userEvent.setup();
  const button = await screen.findByRole("button", { name: "Create a passkey" });
  await waitFor(() => expect(button).not.toHaveAttribute("data-pending"));
  await user.click(button);
}

describe("enrollmentToken", () => {
  it("reads the token from the fragment", () => {
    expect(enrollmentToken(`#token=${token}`)).toBe(token);
    expect(enrollmentToken(`token=${token}`)).toBe(token);
  });

  it.each(["", "#", "#token=", "#other=x"])("finds none in %j", (hash) => {
    expect(enrollmentToken(hash)).toBeUndefined();
  });
});

describe("the enrollment page", () => {
  it("posts the fragment's token in the body only, and offers a passkey", async () => {
    stubWebAuthn();
    const fetch = stubServer({ "POST /auth/local/enrollment/start": started() });
    renderApp(`/enroll#token=${token}`);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Set up your account" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("You will sign in as avery@example.test.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create a passkey" })).toBeInTheDocument();
    const [request] = requestsTo(fetch, "/auth/local/enrollment/start");
    expect(request?.url).not.toContain(token);
    expect(await request?.clone().json()).toEqual({ token });
    await waitFor(() => expect(document.title).toBe("Set up your account · Stepclimb"));
  });

  it("opens a new link pasted into the same tab, where only the fragment changes", async () => {
    stubWebAuthn();
    const fetch = stubServer({ "POST /auth/local/enrollment/start": started() });
    const app = renderApp(`/enroll#token=${token}`);
    await screen.findByRole("button", { name: "Create a passkey" });
    app.router.history.push("/enroll#token=second-token");
    await waitFor(() => expect(requestsTo(fetch, "/auth/local/enrollment/start")).toHaveLength(2));
    const second = requestsTo(fetch, "/auth/local/enrollment/start")[1];
    expect(await second?.clone().json()).toEqual({ token: "second-token" });
  });

  it.each(["/enroll", `/enroll?token=${token}`])(
    "asks for the whole link at %s and sends nothing",
    async (path) => {
      const fetch = stubServer({});
      renderApp(path);
      expect(
        await screen.findByRole("heading", { level: 1, name: "This page needs your setup link" }),
      ).toBeInTheDocument();
      expect(fetch).not.toHaveBeenCalled();
    },
  );

  it("moves the token out of the address bar into the entry's state", async () => {
    stubWebAuthn();
    stubServer({ "POST /auth/local/enrollment/start": started() });
    const app = renderApp(`/enroll#token=${token}`);
    await screen.findByRole("button", { name: "Create a passkey" });
    await waitFor(() => expect(app.router.state.location.hash).toBe(""));
    expect(app.router.state.location.href).toBe("/enroll");
    expect(app.router.history.location.href).not.toContain(token);
    expect(app.router.state.location.state.enrollToken).toBe(token);
    expect(app.router.history.length).toBe(1);
  });

  it("finds the token in the entry's state after a reload", async () => {
    stubWebAuthn();
    const fetch = stubServer({ "POST /auth/local/enrollment/start": started() });
    renderApp("/enroll", { enrollToken: token });
    expect(await screen.findByRole("button", { name: "Create a passkey" })).toBeInTheDocument();
    const [request] = requestsTo(fetch, "/auth/local/enrollment/start");
    expect(await request?.clone().json()).toEqual({ token });
  });

  it("registers the passkey, signs in and leaves the link's entry", async () => {
    const webauthn = stubWebAuthn();
    const fetch = stubServer({
      "POST /auth/local/enrollment/start": started(),
      "POST /auth/local/enrollment/passkey": () => json({ return_to: "/" }),
      "GET /api/v1/me": () => json(meFixture()),
    });
    const app = renderApp(`/enroll#token=${token}`);
    await create();

    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(webauthn.create).toHaveBeenCalledWith({
      publicKey: { parsed: creationOptions.publicKey },
    });
    const [request] = requestsTo(fetch, "/auth/local/enrollment/passkey");
    expect(request?.url).not.toContain(token);
    expect(await request?.clone().json()).toEqual({ token, credential: credentialJson });
    expect(app.router.state.location.href).toBe("/");
    expect(app.router.history.length).toBe(1);
  });

  it("opens / when the server answers a return target the client refuses", async () => {
    stubWebAuthn();
    stubServer({
      "POST /auth/local/enrollment/start": started(),
      "POST /auth/local/enrollment/passkey": () => json({ return_to: "//evil.example" }),
      "GET /api/v1/me": () => json(meFixture()),
    });
    const app = renderApp(`/enroll#token=${token}`);
    await create();
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(app.router.state.location.href).toBe("/");
  });

  it("says when the link can set up only a method the browser cannot", async () => {
    stubWebAuthn();
    stubServer({ "POST /auth/local/enrollment/start": started(["password"]) });
    renderApp(`/enroll#token=${token}`);
    expect(await screen.findByText(/only a password/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create a passkey" })).toBeNull();
  });

  it("says when the browser cannot use passkeys", async () => {
    stubSecureContext(true);
    stubServer({ "POST /auth/local/enrollment/start": started() });
    renderApp(`/enroll#token=${token}`);
    expect(await screen.findByRole("alert")).toHaveTextContent("This browser cannot use passkeys.");
    expect(screen.queryByRole("button", { name: "Create a passkey" })).toBeNull();
  });

  it("says when this device already holds a passkey for the account", async () => {
    const webauthn = stubWebAuthn();
    webauthn.create.mockRejectedValueOnce(domError("InvalidStateError"));
    const fetch = stubServer({ "POST /auth/local/enrollment/start": started() });
    renderApp(`/enroll#token=${token}`);
    await create();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This device already has a passkey for this account.",
    );
    expect(requestsTo(fetch, "/auth/local/enrollment/passkey")).toHaveLength(0);
  });
});

describe("problem codes on the enrollment page (B17, T8)", () => {
  it.each([
    ["auth.enrollment_invalid", "This setup link does not work", "It was already used"],
    ["auth.enrollment_expired", "This setup link has expired", "It can no longer be used."],
  ])("a start answering %s is a dead link, with no retry", async (code, title, message) => {
    stubServer({ "POST /auth/local/enrollment/start": () => problem(400, code, "Bad Request") });
    renderApp(`/enroll#token=${token}`);
    expect(await screen.findByRole("heading", { level: 1, name: title })).toBeInTheDocument();
    expect(screen.getByText(message, { exact: false })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    expect(screen.getByRole("link", { name: "Go to sign in" })).toHaveAttribute("href", "/sign-in");
  });

  it("a start answering auth.method_disabled says so, with no retry", async () => {
    stubServer({
      "POST /auth/local/enrollment/start": () => problem(404, "auth.method_disabled", "Not Found"),
    });
    renderApp(`/enroll#token=${token}`);
    expect(
      await screen.findByText("Your organization has turned off passkey and password sign-in.", {
        exact: false,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("a rate-limited start offers a retry that asks again", async () => {
    stubWebAuthn();
    let calls = 0;
    const fetch = stubServer({
      "POST /auth/local/enrollment/start": () =>
        ++calls === 1 ? problem(429, "auth.rate_limited", "Too Many Requests") : started()(),
    });
    renderApp(`/enroll#token=${token}`);
    expect(
      await screen.findByText("Too many sign-in attempts from your network.", { exact: false }),
    ).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("button", { name: "Create a passkey" })).toBeInTheDocument();
    expect(requestsTo(fetch, "/auth/local/enrollment/start")).toHaveLength(2);
  });

  it("an unknown code shows the code", async () => {
    stubServer({ "POST /auth/local/enrollment/start": () => problem(500, "internal", "Error") });
    renderApp(`/enroll#token=${token}`);
    expect(
      await screen.findByRole("heading", { level: 1, name: "This setup link could not be opened" }),
    ).toBeInTheDocument();
    expect(document.querySelector("code")).toHaveTextContent("internal");
  });

  const registerFails = (status: number, code: string): Record<string, Handler> => ({
    "POST /auth/local/enrollment/start": started(),
    "POST /auth/local/enrollment/passkey": () => problem(status, code, "Refused"),
  });

  it.each([
    [400, "auth.passkey_invalid", "Stepclimb could not register that passkey. Try again."],
    [400, "auth.flow_expired", "The passkey request expired. Try again."],
    [400, "auth.flow_invalid", "This sign-in was already finished"],
  ])(
    "a registration answering %i %s says so and gets new options",
    async (status, code, message) => {
      stubWebAuthn();
      const fetch = stubServer(registerFails(status, code));
      renderApp(`/enroll#token=${token}`);
      await create();
      expect(await screen.findByRole("alert")).toHaveTextContent(message);
      await waitFor(() =>
        expect(requestsTo(fetch, "/auth/local/enrollment/start")).toHaveLength(2),
      );
    },
  );

  it("a registration answering auth.suspended says so", async () => {
    stubWebAuthn();
    stubServer(registerFails(403, "auth.suspended"));
    renderApp(`/enroll#token=${token}`);
    await create();
    expect(await screen.findByRole("alert")).toHaveTextContent("Your account is suspended.");
  });

  it("a retry after a failed registration clears that failure", async () => {
    stubWebAuthn();
    let starts = 0;
    const fetch = stubServer({
      "POST /auth/local/enrollment/start": () =>
        ++starts === 2 ? problem(429, "auth.rate_limited", "Too Many Requests") : started()(),
      "POST /auth/local/enrollment/passkey": () =>
        problem(400, "auth.passkey_invalid", "Bad Request"),
    });
    renderApp(`/enroll#token=${token}`);
    await create();
    // The refused passkey asks for new options, and that start is rate limited.
    const retry = await screen.findByRole("button", { name: "Try again" });
    expect(screen.getByText("Too many sign-in attempts", { exact: false })).toBeInTheDocument();
    await userEvent.setup().click(retry);
    expect(await screen.findByRole("button", { name: "Create a passkey" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(requestsTo(fetch, "/auth/local/enrollment/start")).toHaveLength(3);
  });

  it("a link used up during registration turns the page into the dead-link view", async () => {
    stubWebAuthn();
    stubServer(registerFails(400, "auth.enrollment_invalid"));
    renderApp(`/enroll#token=${token}`);
    await create();
    expect(
      await screen.findByRole("heading", { level: 1, name: "This setup link does not work" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create a passkey" })).toBeNull();
  });
});
