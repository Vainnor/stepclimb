// SPDX-License-Identifier: AGPL-3.0-or-later
// The course routes in jsdom (docs/specs/m1/03-modules-and-items.md B16 to B18) against a stubbed
// server: the deep link's id check and its not-found answers, the sign-in round trip with the
// return URL, the "Back to" link kept for the tab's session, the course home and a module.
import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Item, Module } from "../course/outline";
import { RETURN_KEY } from "../course/return-link";
import {
  type Handler,
  json,
  meFixture,
  problem,
  renderApp,
  requestsTo,
  stubMatchMedia,
  stubServer,
} from "../test/app";

const courseId = "01900000-0000-7000-8000-0000000000c1";
const otherCourseId = "01900000-0000-7000-8000-0000000000c2";
const moduleId = "01900000-0000-7000-8000-0000000000d1";
const otherModuleId = "01900000-0000-7000-8000-0000000000d2";
const itemId = "01900000-0000-7000-8000-0000000000e1";
const ITEM = `/api/v1/items/${itemId}`;
const MODULE = `/api/v1/modules/${moduleId}`;
const MODULES = `/api/v1/courses/${courseId}/modules`;
const DEEP_LINK = `/c/${courseId}/m/${moduleId}/i/${itemId}`;

function itemFixture(overrides: Partial<Item> = {}): Item {
  return {
    id: itemId,
    module_id: moduleId,
    course_id: courseId,
    kind: "external_link",
    title: "Radio phraseology",
    position: 0,
    published: true,
    url: "https://example.org/phraseology",
    requirement: null,
    lock: null,
    created_at: "2026-11-01T12:00:00Z",
    updated_at: "2026-11-01T12:00:00Z",
    ...overrides,
  };
}

function moduleFixture(overrides: Partial<Module> = {}): Module {
  return {
    id: moduleId,
    course_id: courseId,
    title: "Getting started",
    position: 0,
    published: true,
    unlock_at: null,
    prerequisite_ids: [],
    lock: null,
    require_sequential: false,
    completion_rule: "all",
    items: [
      itemFixture({
        id: "01900000-0000-7000-8000-0000000000e0",
        kind: "header",
        title: "Read first",
        url: undefined,
      }),
      itemFixture(),
    ],
    created_at: "2026-11-01T12:00:00Z",
    updated_at: "2026-11-01T12:00:00Z",
    ...overrides,
  };
}

function server(handlers: Record<string, Handler>, origins: string[] = []) {
  return stubServer({
    "GET /api/v1/me": () => json({ ...meFixture(), embedding: { return_origins: origins } }),
    ...handlers,
  });
}

const notFound = () => screen.findByRole("heading", { level: 2, name: /^There is no page at/ });

beforeEach(() => {
  stubMatchMedia(true);
  sessionStorage.clear();
});
afterEach(() => vi.unstubAllGlobals());

describe("the deep link (B16)", () => {
  it("opens the item when its module and course are the path's", async () => {
    server({ [`GET ${ITEM}`]: () => json(itemFixture()) });
    renderApp(DEEP_LINK);
    expect(
      await screen.findByRole("heading", { level: 2, name: "Radio phraseology" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Course" })).toBeInTheDocument();
  });

  it("reads ids in any letter case as the server answers them", async () => {
    server({ [`GET ${ITEM}`]: () => json(itemFixture()) });
    renderApp(
      DEEP_LINK.toUpperCase().replace("/C/", "/c/").replace("/M/", "/m/").replace("/I/", "/i/"),
    );
    expect(
      await screen.findByRole("heading", { level: 2, name: "Radio phraseology" }),
    ).toBeInTheDocument();
  });

  it("shows a locked item's title, as the server answers it, and nothing it withholds", async () => {
    const locked = itemFixture({
      url: undefined,
      lock: { reasons: [{ kind: "unlock_at", at: "2026-12-01T00:00:00Z" }] },
    });
    server({ [`GET ${ITEM}`]: () => json(locked) });
    renderApp(DEEP_LINK);
    expect(
      await screen.findByRole("heading", { level: 2, name: "Radio phraseology" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /^There is no page at/ })).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("example.org");
  });

  it("shows the not-found view for a locked item in another module, as for an open one", async () => {
    const locked = itemFixture({
      module_id: otherModuleId,
      url: undefined,
      lock: { reasons: [{ kind: "module_locked" }] },
    });
    server({ [`GET ${ITEM}`]: () => json(locked) });
    renderApp(DEEP_LINK);
    expect(await notFound()).toBeInTheDocument();
    expect(screen.queryByText("Radio phraseology")).not.toBeInTheDocument();
  });

  it("shows the not-found view when the item is in another module or course (T10)", async () => {
    for (const item of [
      itemFixture({ module_id: otherModuleId }),
      itemFixture({ course_id: otherCourseId }),
    ]) {
      server({ [`GET ${ITEM}`]: () => json(item) });
      renderApp(DEEP_LINK);
      expect(await notFound()).toBeInTheDocument();
      expect(screen.queryByText("Radio phraseology")).not.toBeInTheDocument();
      cleanup();
    }
  });

  it("shows the same not-found view for a 404 and a 403", async () => {
    for (const answer of [
      () => problem(404, "item.not_found", "Not found"),
      () => problem(403, "permission.denied", "Forbidden"),
    ]) {
      server({ [`GET ${ITEM}`]: answer });
      renderApp(DEEP_LINK);
      expect(await notFound()).toBeInTheDocument();
      cleanup();
    }
  });

  it("asks nothing for a path whose ids are no UUIDs", async () => {
    const fetch = server({});
    renderApp(`/c/${courseId}/m/${moduleId}/i/not-an-id`);
    expect(await notFound()).toBeInTheDocument();
    expect(requestsTo(fetch, "/api/v1/items/not-an-id")).toEqual([]);
  });

  it("offers to try again when the read fails for another reason", async () => {
    server({ [`GET ${ITEM}`]: () => problem(500, "internal.error", "Something went wrong") });
    renderApp(DEEP_LINK);
    expect(await screen.findByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("sends a signed-out visit to sign in with the deep link, return URL included (B18)", async () => {
    stubServer({
      "GET /api/v1/me": () => problem(401, "auth.unauthenticated", "Sign in"),
    });
    const target = `${DEEP_LINK}?return=${encodeURIComponent("https://host.example/x")}`;
    const app = renderApp(target);
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(app.router.state.location.pathname).toBe("/sign-in");
    const returnTo = app.router.state.location.search.return_to ?? "";
    const back = new URL(returnTo, "http://localhost");
    expect(back.pathname).toBe(DEEP_LINK);
    expect(back.searchParams.get("return")).toBe("https://host.example/x");
  });
});

describe("the return link (B17)", () => {
  const withReturn = (url: string) => `${DEEP_LINK}?return=${encodeURIComponent(url)}`;

  it("shows Back to the host of a URL on a listed origin, and keeps it for the tab (T11)", async () => {
    server({ [`GET ${ITEM}`]: () => json(itemFixture()) }, ["https://host.example"]);
    renderApp(withReturn("https://host.example/x"));
    const link = await screen.findByRole("link", { name: /^Back to host\.example/ });
    expect(link).toHaveAttribute("href", "https://host.example/x");
    expect(
      screen.getByRole("navigation", { name: "Return to the site that sent you here" }),
    ).toContainElement(link);
    expect(sessionStorage.getItem(RETURN_KEY)).toBe("https://host.example/x");
  });

  it("shows nothing for an origin that is not listed, and drops the kept one (T11)", async () => {
    sessionStorage.setItem(RETURN_KEY, "https://host.example/kept");
    server({ [`GET ${ITEM}`]: () => json(itemFixture()) }, ["https://host.example"]);
    const app = renderApp(withReturn("https://other.example/"));
    await screen.findByRole("heading", { level: 2, name: "Radio phraseology" });
    expect(screen.queryByRole("link", { name: /^Back to/ })).not.toBeInTheDocument();
    expect(sessionStorage.getItem(RETURN_KEY)).toBeNull();
    expect(app.router.state.location.pathname).toBe(DEEP_LINK);
  });

  it("shows the kept link on a course page whose address has none", async () => {
    sessionStorage.setItem(RETURN_KEY, "https://host.example/kept");
    server({ [`GET ${MODULE}`]: () => json(moduleFixture()) }, ["https://host.example"]);
    renderApp(`/c/${courseId}/m/${moduleId}`);
    const link = await screen.findByRole("link", { name: /^Back to host\.example/ });
    expect(link).toHaveAttribute("href", "https://host.example/kept");
  });

  it("hides the kept link once the organization no longer lists its origin", async () => {
    sessionStorage.setItem(RETURN_KEY, "https://host.example/kept");
    server({ [`GET ${MODULE}`]: () => json(moduleFixture()) }, []);
    renderApp(`/c/${courseId}/m/${moduleId}`);
    await screen.findByRole("heading", { level: 2, name: "Getting started" });
    expect(screen.queryByRole("link", { name: /^Back to/ })).not.toBeInTheDocument();
  });

  it("treats a return that is not a string as refused", async () => {
    sessionStorage.setItem(RETURN_KEY, "https://host.example/kept");
    server({ [`GET ${ITEM}`]: () => json(itemFixture()) }, ["https://host.example"]);
    renderApp(`${DEEP_LINK}?return=1`);
    await screen.findByRole("heading", { level: 2, name: "Radio phraseology" });
    expect(screen.queryByRole("link", { name: /^Back to/ })).not.toBeInTheDocument();
    expect(sessionStorage.getItem(RETURN_KEY)).toBeNull();
  });
});

describe("the course home and a module (B16)", () => {
  it("lists the modules and links each item to its deep link", async () => {
    server({ [`GET ${MODULES}`]: () => json({ modules: [moduleFixture()] }) });
    renderApp(`/c/${courseId}`);
    const module = await screen.findByRole("link", { name: "Getting started" });
    expect(module).toHaveAttribute("href", `/c/${courseId}/m/${moduleId}`);
    expect(screen.getByRole("link", { name: "Radio phraseology" })).toHaveAttribute(
      "href",
      DEEP_LINK,
    );
    expect(screen.getByRole("heading", { level: 3, name: "Read first" })).toBeInTheDocument();
  });

  it("says when a course has no modules to open", async () => {
    server({ [`GET ${MODULES}`]: () => json({ modules: [] }) });
    renderApp(`/c/${courseId}`);
    expect(
      await screen.findByText("This course has no modules you can open yet."),
    ).toBeInTheDocument();
  });

  it("shows the not-found view for a course the server does not show", async () => {
    server({ [`GET ${MODULES}`]: () => problem(404, "course.not_found", "Not found") });
    renderApp(`/c/${courseId}`);
    expect(await notFound()).toBeInTheDocument();
  });

  it("shows a module with its items, and the not-found view for one of another course", async () => {
    server({ [`GET ${MODULE}`]: () => json(moduleFixture()) });
    renderApp(`/c/${courseId}/m/${moduleId}`);
    expect(
      await screen.findByRole("heading", { level: 2, name: "Getting started" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Radio phraseology" })).toHaveAttribute(
      "href",
      DEEP_LINK,
    );
    cleanup();

    server({ [`GET ${MODULE}`]: () => json(moduleFixture({ course_id: otherCourseId })) });
    renderApp(`/c/${courseId}/m/${moduleId}`);
    expect(await notFound()).toBeInTheDocument();
    expect(screen.queryByText("Getting started")).not.toBeInTheDocument();
  });
});
