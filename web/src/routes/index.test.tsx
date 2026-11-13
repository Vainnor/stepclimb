// SPDX-License-Identifier: AGPL-3.0-or-later
import { StringsProvider } from "@stepclimb/ui";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogs } from "../messages";
import { HealthRoute } from "./index";

function renderRoute() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <StringsProvider catalogs={catalogs} isDevelopment>
      <QueryClientProvider client={client}>
        <HealthRoute />
      </QueryClientProvider>
    </StringsProvider>,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe("HealthRoute", () => {
  it("shows the status /healthz returns, fetched through the API client", async () => {
    const fetch = vi.fn(async (_req: Request) =>
      Response.json({ status: "ok" }, { headers: { "content-type": "application/json" } }),
    );
    vi.stubGlobal("fetch", fetch);

    renderRoute();

    expect(await screen.findByText("ok")).toBeInTheDocument();
    expect(fetch).toHaveBeenCalledTimes(1);
    const request = fetch.mock.calls[0]?.[0] as Request;
    expect(new URL(request.url).pathname).toBe("/healthz");
    expect(request.method).toBe("GET");
  });

  it("says the server did not answer when the request fails", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Promise.reject(new TypeError("network down"))),
    );

    renderRoute();

    expect(await screen.findByText(/did not answer/)).toBeInTheDocument();
  });
});
