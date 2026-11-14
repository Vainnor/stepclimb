// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: the whole app on a memory history, against a stubbed `fetch` that answers by
// method and path. jsdom has no `matchMedia`; every query answers `wide`, so a test picks the
// SideNav (wide) or the BottomTabs layout.
import { StringsProvider } from "@stepclimb/ui";
import { createMemoryHistory } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import { vi } from "vitest";
import { type App, AppRoot, createApp } from "../app";
import { catalogs } from "../messages";
import type { Me } from "../session";

export type Handler = (request: Request) => Response | Promise<Response>;

/** Stubs `fetch`; a request no handler names fails the test. Returns the mock. */
export function stubServer(handlers: Record<string, Handler>) {
  const fetch = vi.fn(async (request: Request) => {
    const key = `${request.method} ${new URL(request.url).pathname}`;
    const handler = handlers[key];
    if (!handler) throw new Error(`unexpected request: ${key}`);
    return handler(request);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
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

export function renderApp(path: string): App {
  const app = createApp(createMemoryHistory({ initialEntries: [path] }));
  render(
    <StringsProvider catalogs={catalogs} isDevelopment>
      <AppRoot app={app} />
    </StringsProvider>,
  );
  return app;
}
