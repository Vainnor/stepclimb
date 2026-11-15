// SPDX-License-Identifier: AGPL-3.0-or-later
// The shell's error and offline states in jsdom (docs/specs/m0/11-app-shell.md B28, B29, T12).
// M0's one signed-in view loads nothing, so this file replaces Home with a view that loads
// `/api/v1/organization` through a suspense query, the way a view with data does. It also gives
// the enrollment view and the signed-in layout's navigation a fault, to show where those errors
// are drawn. The rest of the route tree, the layouts and the query client are the app's own.
import { onlineManager, useSuspenseQuery } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, unwrap } from "./api";
import { json, meFixture, problem, renderApp, stubMatchMedia, stubServer } from "./test/app";

const view = vi.hoisted(() => ({ fault: false, layoutFault: false }));

vi.mock("./routes/home", () => ({
  HomeView: function LoadingHome() {
    if (view.fault) throw new Error("a fault in the view");
    const organization = useSuspenseQuery({
      queryKey: ["organization"],
      queryFn: async () => unwrap(await api.GET("/api/v1/organization")),
    });
    return <p>{`loaded ${organization.data.name}`}</p>;
  },
}));

// The signed-in layout draws its navigation through this module; `layoutFault` breaks it.
vi.mock("./navigation", async (actual) => {
  const navigation = await actual<typeof import("./navigation")>();
  return {
    ...navigation,
    visibleNavEntries: (...args: Parameters<typeof navigation.visibleNavEntries>) => {
      if (view.layoutFault) throw new Error("a fault in the layout");
      return navigation.visibleNavEntries(...args);
    },
  };
});

// A signed-out view with a fault, to show where its error is drawn.
vi.mock("./routes/enroll", () => ({
  EnrollView: function FaultyEnroll(): never {
    throw new Error("a fault in the view");
  },
}));

const signedIn = () => json(meFixture());
const organization = () => json(meFixture().organization);
const unreachable = () => Promise.reject(new TypeError("Failed to fetch"));

/** Answers each call with the next response in `answers`, the last one for good. */
function inTurn(...answers: (() => Response | Promise<Response>)[]) {
  let calls = 0;
  const handler = () => {
    const answer = answers[Math.min(calls, answers.length - 1)] as () => Response;
    calls += 1;
    return answer();
  };
  return Object.assign(handler, { calls: () => calls });
}

const sideNav = () => document.querySelector('[data-slot="side-nav"]');

beforeEach(() => {
  stubMatchMedia(true);
  view.fault = false;
  view.layoutFault = false;
});
afterEach(() => {
  vi.unstubAllGlobals();
  onlineManager.setOnline(true);
});

describe("a route that fails to load (B28, T12)", () => {
  it("draws the server's title inside AppShell, and Try again refetches", async () => {
    const org = inTurn(() => problem(500, "internal", "Internal Server Error"), organization);
    stubServer({ "GET /api/v1/me": signedIn, "GET /api/v1/organization": org });
    renderApp("/");

    expect(
      await screen.findByRole("heading", { level: 2, name: "Internal Server Error" }),
    ).toBeInTheDocument();
    // The shell stays: the view's title, the navigation and the user menu.
    expect(screen.getByRole("heading", { level: 1, name: "Home" })).toBeInTheDocument();
    expect(within(sideNav() as HTMLElement).getByRole("link", { name: "Home" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Avery Example, account menu" })).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("loaded Example Training")).toBeInTheDocument();
    expect(org.calls()).toBe(2);
    expect(document.querySelector('[data-slot="error-state"]')).toBeNull();
  });

  it("says the server could not be reached when it does not answer, and stays signed in", async () => {
    const org = inTurn(unreachable, organization);
    stubServer({ "GET /api/v1/me": signedIn, "GET /api/v1/organization": org });
    const app = renderApp("/");

    expect(
      await screen.findByRole("heading", { level: 2, name: "The server could not be reached" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Check your connection, then try again.")).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/");
    expect(sideNav()).toBeInTheDocument();
    // Nothing was drawn before, so there is nothing stale to warn about.
    expect(document.querySelector('[data-slot="unreachable-notice"]')).toBeEmptyDOMElement();

    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("loaded Example Training")).toBeInTheDocument();
  });

  it("does not call a fault in the page an unreachable server", async () => {
    view.fault = true;
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    stubServer({ "GET /api/v1/me": signedIn });
    renderApp("/");

    expect(await screen.findByRole("heading", { level: 2, name: "Could not load" })).toBeVisible();
    expect(
      screen.getByText("Stepclimb could not load this page. Try again in a moment."),
    ).toBeInTheDocument();
    expect(screen.queryByText("The server could not be reached")).toBeNull();
    expect(sideNav()).toBeInTheDocument();
  });

  it("draws a signed-out view's error in the signed-out layout, as the page's h1", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    stubServer({});
    renderApp("/enroll");

    expect(await screen.findByRole("heading", { level: 1, name: "Could not load" })).toBeVisible();
    expect(document.querySelectorAll('[data-slot="signed-out"]')).toHaveLength(1);
    expect(document.querySelectorAll("main")).toHaveLength(1);
    expect(document.querySelector('[data-slot="app-shell"]')).toBeNull();
    await waitFor(() => expect(document.title).toBe("Could not load · Stepclimb"));
  });

  it("draws a fault in a layout itself in the signed-out frame", async () => {
    view.layoutFault = true;
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    stubServer({ "GET /api/v1/me": signedIn, "GET /api/v1/organization": organization });
    renderApp("/");

    expect(await screen.findByRole("heading", { level: 1, name: "Could not load" })).toBeVisible();
    expect(document.querySelectorAll('[data-slot="signed-out"]')).toHaveLength(1);
    expect(document.querySelectorAll("main")).toHaveLength(1);
    expect(document.querySelector('[data-slot="app-shell"]')).toBeNull();
  });

  it("draws no error for a 401 and goes to sign in", async () => {
    stubServer({
      "GET /api/v1/me": inTurn(signedIn, () =>
        problem(401, "auth.unauthenticated", "Unauthorized"),
      ),
      "GET /api/v1/organization": () => problem(401, "auth.unauthenticated", "Unauthorized"),
    });
    // Records whether an error view was ever drawn, however briefly, on the way to sign in.
    let errorDrawn = false;
    const observer = new MutationObserver(() => {
      errorDrawn ||= document.querySelector('[data-slot="error-state"]') !== null;
    });
    observer.observe(document.body, { childList: true, subtree: true });
    const app = renderApp("/");
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    observer.disconnect();
    expect(errorDrawn).toBe(false);
    expect(app.router.state.location.search).toEqual({ return_to: "/" });
  });
});

describe("the offline state (B29, T12)", () => {
  /** Refetches every query, as a tab coming back into view does. */
  async function refetchAll(app: ReturnType<typeof renderApp>) {
    await act(async () => {
      await app.context.queryClient.refetchQueries();
    });
  }

  it("keeps the page and says the server could not be reached when a refetch gets no answer", async () => {
    let down = false;
    stubServer({
      "GET /api/v1/me": () => (down ? unreachable() : signedIn()),
      "GET /api/v1/organization": () => (down ? unreachable() : organization()),
    });
    const app = renderApp("/");
    await screen.findByText("loaded Example Training");
    const notice = document.querySelector('[data-slot="unreachable-notice"]');
    expect(notice).toHaveAttribute("role", "status");
    expect(notice).toBeEmptyDOMElement();

    down = true;
    await refetchAll(app);
    expect(
      await screen.findByText(
        "The server could not be reached, so this page may be out of date. Check your connection, then try again.",
      ),
    ).toBeInTheDocument();
    // What was loaded stays, and so does the session.
    expect(screen.getByText("loaded Example Training")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Avery Example, account menu" })).toBeVisible();
    expect(app.router.state.location.pathname).toBe("/");

    down = false;
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(notice).toBeEmptyDOMElement());
  });

  it("goes by itself when the browser's network returns and the queries refetch", async () => {
    // `/me` is stale at once and refetches when the network returns; a suspense query keeps its
    // data fresh for a second, so this test takes only `/me` down.
    let down = false;
    stubServer({
      "GET /api/v1/me": () => (down ? unreachable() : signedIn()),
      "GET /api/v1/organization": organization,
    });
    const app = renderApp("/");
    await screen.findByText("loaded Example Training");
    down = true;
    await act(async () => onlineManager.setOnline(false));
    await act(async () => {
      await app.context.queryClient.refetchQueries({ queryKey: ["me"] });
    });
    await screen.findByRole("button", { name: "Try again" });

    down = false;
    await act(async () => onlineManager.setOnline(true));
    await waitFor(() =>
      expect(document.querySelector('[data-slot="unreachable-notice"]')).toBeEmptyDOMElement(),
    );
  });

  it("asks the server even when the browser says it is offline", async () => {
    onlineManager.setOnline(false);
    const me = inTurn(unreachable, signedIn);
    stubServer({ "GET /api/v1/me": me, "GET /api/v1/organization": organization });
    renderApp("/");
    expect(
      await screen.findByRole("heading", { level: 1, name: "The server could not be reached" }),
    ).toBeInTheDocument();
    expect(me.calls()).toBe(1);
    // A server on the same machine answers whatever the browser thinks of its network.
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("loaded Example Training")).toBeInTheDocument();
  });

  it("draws no notice for a query that failed with an answer", async () => {
    let failing = false;
    stubServer({
      "GET /api/v1/me": () =>
        failing ? problem(503, "internal", "Service Unavailable") : signedIn(),
      "GET /api/v1/organization": organization,
    });
    const app = renderApp("/");
    await screen.findByText("loaded Example Training");
    failing = true;
    await refetchAll(app);
    expect(document.querySelector('[data-slot="unreachable-notice"]')).toBeEmptyDOMElement();
  });
});
