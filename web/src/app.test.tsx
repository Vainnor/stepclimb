// SPDX-License-Identifier: AGPL-3.0-or-later
// The shell end to end in jsdom: routing, layouts, `/me`, session expiry and sign-out
// (docs/specs/m0/11-app-shell.md T1 to T5, T17, T18).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { json, meFixture, problem, renderApp, stubMatchMedia, stubServer } from "./test/app";

const unauthenticated = () => problem(401, "auth.unauthenticated", "Unauthorized");
const signedIn = () => json(meFixture());

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("signed-in layout", () => {
  it("draws Home inside AppShell once /me has loaded (B2, B4, B6)", async () => {
    stubServer({ "GET /api/v1/me": signedIn });
    renderApp("/");

    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    const nav = document.querySelector<HTMLElement>('[data-slot="side-nav"]');
    expect(nav).not.toBeNull();
    expect(within(nav as HTMLElement).getByRole("link", { name: "Home" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("button", { name: "Avery Example, account menu" })).toBeVisible();
    expect(screen.getByRole("link", { name: "Skip to main content" })).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Home · Example Training"));
  });

  it("asks /me through the API client with the browser's cookie", async () => {
    const fetch = stubServer({ "GET /api/v1/me": signedIn });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    const request = fetch.mock.calls[0]?.[0] as Request;
    expect(new URL(request.url).pathname).toBe("/api/v1/me");
    expect(request.method).toBe("GET");
  });
});

describe("the /me load (B9, T3)", () => {
  function pendingMe() {
    let answer: (response: Response) => void = () => undefined;
    const response = new Promise<Response>((resolve) => {
      answer = resolve;
    });
    stubServer({ "GET /api/v1/me": () => response });
    return (r: Response) => answer(r);
  }

  const skeleton = () => document.querySelector('[data-slot="skeleton"]');

  // The shell's outline is up once the router has rendered; the Skeleton's 300 ms count from there.
  async function shellRendered() {
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(document.querySelector('[data-slot="shell-skeleton"]')).toBeInTheDocument();
  }

  it("draws no Skeleton for an answer within 300 ms", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const answer = pendingMe();
    renderApp("/");
    await shellRendered();
    await act(() => vi.advanceTimersByTimeAsync(299));
    expect(skeleton()).toBeNull();
    await act(async () => answer(signedIn()));
    await act(() => vi.advanceTimersByTimeAsync(10));
    expect(screen.getByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(skeleton()).toBeNull();
  });

  it("draws the Skeleton once the load has taken 300 ms", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const answer = pendingMe();
    renderApp("/");
    await shellRendered();
    await act(() => vi.advanceTimersByTimeAsync(300));
    expect(skeleton()).toBeInTheDocument();
    await act(async () => answer(signedIn()));
    await act(() => vi.advanceTimersByTimeAsync(10));
    expect(screen.getByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(skeleton()).toBeNull();
  });

  it("offers to try again when /me fails without a 401, and does not sign out", async () => {
    let calls = 0;
    stubServer({
      "GET /api/v1/me": () => {
        calls += 1;
        return calls === 1 ? problem(503, "org.none", "Service Unavailable") : signedIn();
      },
    });
    const app = renderApp("/");
    expect(
      await screen.findByRole("heading", { level: 1, name: "Service Unavailable" }),
    ).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/");
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
  });

  it("says the server could not be reached when it does not answer", async () => {
    stubServer({ "GET /api/v1/me": () => Promise.reject(new TypeError("network down")) });
    const app = renderApp("/");
    expect(
      await screen.findByRole("heading", { level: 1, name: "The server could not be reached" }),
    ).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/");
  });
});

describe("session expiry (B10, T4)", () => {
  it("sends a 401 from /me to /sign-in with the requested path as return target", async () => {
    stubServer({ "GET /api/v1/me": unauthenticated });
    const app = renderApp("/?tab=2");
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/sign-in");
    expect(app.router.state.location.search).toEqual({ return_to: "/?tab=2" });
  });

  it("draws no error for the 401 on its way to the sign-in screen", async () => {
    stubServer({ "GET /api/v1/me": unauthenticated });
    const seen: string[] = [];
    const observer = new MutationObserver(() => {
      for (const heading of document.querySelectorAll("h1")) seen.push(heading.textContent ?? "");
    });
    observer.observe(document.body, { childList: true, subtree: true, characterData: true });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Sign in" });
    observer.disconnect();
    expect(new Set(seen)).toEqual(new Set(["Sign in"]));
  });

  it("on a later 401, clears the cache and leaves no previous user's data drawn", async () => {
    let expired = false;
    const fetch = stubServer({
      "GET /api/v1/me": () => (expired ? unauthenticated() : signedIn()),
    });
    const app = renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(screen.getByText("Avery Example")).toBeInTheDocument();

    expired = true;
    await act(() => app.context.queryClient.invalidateQueries());

    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.search).toEqual({ return_to: "/" });
    expect(screen.queryByText("Avery Example")).toBeNull();
    expect(screen.queryByText("Example Training")).toBeNull();
    expect(app.context.queryClient.getQueryCache().getAll()).toEqual([]);
    // One expired /me, not a loop of them.
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("leaves once when several requests answer 401 together", async () => {
    stubServer({
      "GET /api/v1/me": signedIn,
      "GET /api/v1/organization": unauthenticated,
      "GET /api/v1/organization/settings": unauthenticated,
    });
    const app = renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    const navigate = vi.spyOn(app.router, "navigate");
    const { api, unwrap } = await import("./api");
    await act(async () => {
      await Promise.allSettled([
        app.context.queryClient.fetchQuery({
          queryKey: ["organization"],
          queryFn: async () => unwrap(await api.GET("/api/v1/organization")),
        }),
        app.context.queryClient.fetchQuery({
          queryKey: ["settings"],
          queryFn: async () => unwrap(await api.GET("/api/v1/organization/settings")),
        }),
      ]);
    });
    await screen.findByRole("heading", { level: 1, name: "Sign in" });
    expect(navigate).toHaveBeenCalledTimes(1);
  });

  it("treats a 401 from any other query made while signed in the same way", async () => {
    stubServer({
      "GET /api/v1/me": signedIn,
      "GET /api/v1/organization": unauthenticated,
    });
    const app = renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    const { api, unwrap } = await import("./api");
    await act(async () => {
      await app.context.queryClient
        .fetchQuery({
          queryKey: ["organization"],
          queryFn: async () => unwrap(await api.GET("/api/v1/organization")),
        })
        .catch(() => undefined);
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.context.queryClient.getQueryCache().getAll()).toEqual([]);
  });
});

describe("sign-out (B12, T5)", () => {
  async function signOutFromMenu() {
    await userEvent.click(
      await screen.findByRole("button", { name: "Avery Example, account menu" }),
    );
    await userEvent.click(await screen.findByRole("menuitem", { name: "Sign out" }));
  }

  it("posts to /auth/sign-out, clears the cache and goes to the sign-in screen", async () => {
    let signedOut = false;
    const fetch = stubServer({
      "GET /api/v1/me": () => (signedOut ? unauthenticated() : signedIn()),
      "POST /auth/sign-out": () => {
        signedOut = true;
        return new Response(null, { status: 204 });
      },
    });
    const app = renderApp("/");
    await signOutFromMenu();

    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.search).toEqual({});
    expect(screen.queryByText("Avery Example")).toBeNull();
    expect(app.context.queryClient.getQueryCache().getAll()).toEqual([]);
    const posts = fetch.mock.calls.filter(([request]) => request.method === "POST");
    expect(posts).toHaveLength(1);
  });

  it("shows an error toast and stays signed in when sign-out fails", async () => {
    stubServer({
      "GET /api/v1/me": signedIn,
      "POST /auth/sign-out": () => problem(403, "auth.csrf_refused", "Forbidden"),
    });
    const app = renderApp("/");
    await signOutFromMenu();

    const toast = await screen.findByRole("alertdialog");
    expect(within(toast).getByText("Sign-out failed")).toBeInTheDocument();
    expect(
      within(toast).getByText("You are still signed in. Try again in a moment."),
    ).toBeVisible();
    expect(screen.getByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/");
  });

  it("shows the same toast when the server does not answer", async () => {
    stubServer({
      "GET /api/v1/me": signedIn,
      "POST /auth/sign-out": () => Promise.reject(new TypeError("network down")),
    });
    renderApp("/");
    await signOutFromMenu();
    expect(await screen.findByText("Sign-out failed")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
  });
});

describe("not found (B7, B27, T18)", () => {
  it("draws the deep link as not found inside AppShell when signed in", async () => {
    stubServer({ "GET /api/v1/me": signedIn });
    const app = renderApp("/c/example/m/1/i/1");
    expect(
      await screen.findByRole("heading", { level: 1, name: "Page not found" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 2, name: "There is no page at /c/example/m/1/i/1" }),
    ).toBeInTheDocument();
    expect(document.querySelector('[data-slot="side-nav"]')).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/c/example/m/1/i/1");
    await waitFor(() => expect(document.title).toBe("Page not found · Example Training"));
  });

  it("draws it in the signed-out layout, without sending the browser to sign in", async () => {
    stubServer({ "GET /api/v1/me": unauthenticated });
    const app = renderApp("/nowhere");
    expect(
      await screen.findByRole("heading", { level: 1, name: "There is no page at /nowhere" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).toBeNull();
    expect(app.router.state.location.pathname).toBe("/nowhere");
  });
});

describe("client-side navigation (B1, B6, T1, T17)", () => {
  it("follows a Stepclimb UI link through the router and focuses the new view's h1", async () => {
    stubServer({ "GET /api/v1/me": signedIn });
    const app = renderApp("/nowhere");
    const home = await screen.findByRole("link", { name: "Go to the home page" });
    const navigated = vi.fn();
    const unsubscribe = app.router.subscribe("onResolved", navigated);

    await userEvent.click(home);

    const heading = await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(app.router.state.location.pathname).toBe("/");
    expect(navigated).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(heading).toHaveFocus());
    expect(heading).toHaveAttribute("tabindex", "-1");
    await waitFor(() => expect(document.title).toBe("Home · Example Training"));
    unsubscribe();
  });

  it("leaves focus alone on the first page load", async () => {
    stubServer({ "GET /api/v1/me": signedIn });
    renderApp("/");
    const heading = await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(heading).not.toHaveFocus();
  });
});

describe("narrow layout (B3)", () => {
  it("draws BottomTabs with the same entries", async () => {
    stubMatchMedia(false);
    stubServer({ "GET /api/v1/me": signedIn });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    const tabs = document.querySelector('[data-slot="bottom-tabs"]');
    expect(tabs).not.toBeNull();
    expect(within(tabs as HTMLElement).getByRole("link", { name: "Home" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });
});
