// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: the whole app on a memory history, against a stubbed `fetch` that answers by
// method and path. jsdom has no `matchMedia`; every query answers `wide`, so a test picks the
// SideNav (wide) or the BottomTabs layout.
import { StringsProvider } from "@stepclimb/ui";
import { createMemoryHistory, type HistoryState } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import { vi } from "vitest";
import { type App, AppRoot, createApp } from "../app";
import { catalogs } from "../messages";
import type { Me } from "../session";
import type { Discovery, LocalMethod, SignInMethod } from "../sign-in/discovery";

export type Handler = (request: Request) => Response | Promise<Response>;

/**
 * A synthetic discovery document: Example Training offering local sign-in with `methods`, then
 * `providers`.
 */
export function discoveryFixture(
  methods: LocalMethod[] | null = ["passkey"],
  providers: SignInMethod[] = [],
): Discovery {
  const local: SignInMethod[] =
    methods === null ? [] : [{ key: "local", kind: "local", label: null, methods }];
  return {
    name: "Example Training",
    sign_in: [...local, ...providers],
    theme: { stylesheet: "http://localhost/theme.css" },
  };
}

const DISCOVERY = "GET /.well-known/stepclimb";

/**
 * Stubs `fetch`; a request no handler names fails the test. Returns the mock. The signed-out
 * layout reads the discovery document, so a test that does not answer it itself gets
 * `discoveryFixture()`; `requestsTo` and `apiRequests` leave that request out.
 */
export function stubServer(handlers: Record<string, Handler>) {
  const all: Record<string, Handler> = { [DISCOVERY]: () => json(discoveryFixture()), ...handlers };
  const fetch = vi.fn(async (request: Request) => {
    const key = `${request.method} ${new URL(request.url).pathname}`;
    const handler = all[key];
    if (!handler) throw new Error(`unexpected request: ${key}`);
    return handler(request);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

/** The requests the stub got, but for the discovery document's. */
export function apiRequests(fetch: ReturnType<typeof stubServer>): Request[] {
  return fetch.mock.calls
    .map(([request]) => request as Request)
    .filter((request) => `${request.method} ${new URL(request.url).pathname}` !== DISCOVERY);
}

/** The requests the stub got for `path`. */
export function requestsTo(fetch: ReturnType<typeof stubServer>, path: string): Request[] {
  return fetch.mock.calls
    .map(([request]) => request as Request)
    .filter((request) => new URL(request.url).pathname === path);
}

export function stubMatchMedia(wide: boolean): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: wide,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  }));
}

export const orgId = "01900000-0000-7000-8000-000000000001";

/** A synthetic `/me` answer; `permissions` are held at organization scope. */
export function meFixture(permissions: string[] = []): Me {
  return {
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
    grants:
      permissions.length === 0 ? [] : [{ scope: { type: "organization", id: orgId }, permissions }],
    features: [],
    appearance: { theme: "system", density: "comfortable" },
  };
}

export function json(body: unknown, status = 200): Response {
  return Response.json(body, { status });
}

export function problem(status: number, code: string, title: string): Response {
  return new Response(JSON.stringify({ type: "about:blank", title, status, detail: title, code }), {
    status,
    headers: { "content-type": "application/problem+json" },
  });
}

/** Renders the app at `path`; `state` is the history entry's, as after a reload of that entry. */
export function renderApp(path: string, state?: HistoryState): App {
  const history = createMemoryHistory({ initialEntries: [path] });
  if (state !== undefined) history.replace(path, state);
  const app = createApp(history);
  render(
    <StringsProvider catalogs={catalogs} isDevelopment>
      <AppRoot app={app} />
    </StringsProvider>,
  );
  return app;
}
