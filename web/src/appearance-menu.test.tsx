// SPDX-License-Identifier: AGPL-3.0-or-later
// The appearance choice in the user menu and the OrgSwitcher's absence
// (docs/specs/m0/11-app-shell.md B19, B21, T9).
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ThemeChoice, UserAppearance } from "./appearance";
import { meQuery } from "./session";
import { json, meFixture, problem, renderApp, stubMatchMedia, stubServer } from "./test/app";

const html = document.documentElement;

function signedIn(
  appearance: UserAppearance = { theme: "system", density: "comfortable" },
  organizationDefault: ThemeChoice = "system",
) {
  return () => json({ ...meFixture(), appearance, default_appearance: organizationDefault });
}

async function choose(submenu: string, item: string) {
  await userEvent.click(await screen.findByRole("button", { name: "Avery Example, account menu" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: submenu }));
  const menu = await screen.findByRole("menu", { name: submenu });
  await userEvent.click(within(menu).getByRole("menuitemradio", { name: item }));
}

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.unstubAllGlobals();
  html.removeAttribute("data-sc-theme");
  html.removeAttribute("data-sc-density");
});

describe("the appearance choice (B21)", () => {
  it("applies the choice /me returns to the page", async () => {
    stubServer({ "GET /api/v1/me": signedIn({ theme: "dark", density: "compact" }) });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await waitFor(() => expect(html.getAttribute("data-sc-theme")).toBe("dark"));
    expect(html.getAttribute("data-sc-density")).toBe("compact");
  });

  it("marks the current theme and saves another on the server before applying it", async () => {
    const bodies: unknown[] = [];
    stubServer({
      "GET /api/v1/me": signedIn(),
      "PUT /api/v1/me/appearance": async (request) => {
        const body = await request.json();
        bodies.push(body);
        return json(body);
      },
    });
    const app = renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await userEvent.click(screen.getByRole("button", { name: "Avery Example, account menu" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Theme" }));
    const menu = await screen.findByRole("menu", { name: "Theme" });
    expect(within(menu).getByRole("menuitemradio", { name: "Match my device" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await userEvent.click(within(menu).getByRole("menuitemradio", { name: "High contrast" }));

    await waitFor(() => expect(html.getAttribute("data-sc-theme")).toBe("high-contrast"));
    expect(bodies).toEqual([{ theme: "high-contrast", density: "comfortable" }]);
    expect(app.context.queryClient.getQueryData(meQuery.queryKey)?.appearance).toEqual({
      theme: "high-contrast",
      density: "comfortable",
    });
  });

  it("saves a density with the current theme", async () => {
    const bodies: unknown[] = [];
    stubServer({
      "GET /api/v1/me": signedIn({ theme: "light", density: "comfortable" }),
      "PUT /api/v1/me/appearance": async (request) => {
        const body = await request.json();
        bodies.push(body);
        return json(body);
      },
    });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await choose("Density", "Compact");
    await waitFor(() => expect(html.getAttribute("data-sc-density")).toBe("compact"));
    expect(html.getAttribute("data-sc-theme")).toBe("light");
    expect(bodies).toEqual([{ theme: "light", density: "compact" }]);
  });

  it("returns to following the device for Match my device", async () => {
    stubServer({
      "GET /api/v1/me": signedIn({ theme: "dark", density: "comfortable" }),
      "PUT /api/v1/me/appearance": async (request) => json(await request.json()),
    });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await waitFor(() => expect(html.getAttribute("data-sc-theme")).toBe("dark"));
    await choose("Theme", "Match my device");
    await waitFor(() => expect(html.hasAttribute("data-sc-theme")).toBe(false));
  });

  it("takes the organization's default until the user chooses, and marks it", async () => {
    const bodies: unknown[] = [];
    stubServer({
      "GET /api/v1/me": signedIn({ theme: null, density: "comfortable" }, "dark"),
      "PUT /api/v1/me/appearance": async (request) => {
        const body = await request.json();
        bodies.push(body);
        return json(body);
      },
    });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await waitFor(() => expect(html.getAttribute("data-sc-theme")).toBe("dark"));
    await userEvent.click(screen.getByRole("button", { name: "Avery Example, account menu" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Theme" }));
    const menu = await screen.findByRole("menu", { name: "Theme" });
    expect(within(menu).getByRole("menuitemradio", { name: "Dark" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await userEvent.keyboard("{Escape}{Escape}");

    // A density change leaves the theme unchosen.
    await choose("Density", "Compact");
    await waitFor(() => expect(html.getAttribute("data-sc-density")).toBe("compact"));
    expect(html.getAttribute("data-sc-theme")).toBe("dark");
    expect(bodies).toEqual([{ theme: null, density: "compact" }]);

    // The user's own choice wins over the default.
    await choose("Theme", "Match my device");
    await waitFor(() => expect(html.hasAttribute("data-sc-theme")).toBe(false));
    expect(bodies[1]).toEqual({ theme: "system", density: "compact" });
  });

  it("blocks a second choice while the first is saving, and marks the one being saved", async () => {
    const bodies: unknown[] = [];
    let answer: (response: Response) => void = () => undefined;
    stubServer({
      "GET /api/v1/me": signedIn(),
      "PUT /api/v1/me/appearance": async (request) => {
        bodies.push(await request.json());
        return new Promise<Response>((resolve) => {
          answer = resolve;
        });
      },
    });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await choose("Theme", "Dark");
    await waitFor(() => expect(bodies).toHaveLength(1));

    // Compact, before Dark's save has answered.
    if (!screen.queryByRole("menuitem", { name: /^Density/ })) {
      await userEvent.click(screen.getByRole("button", { name: "Avery Example, account menu" }));
    }
    const density = await screen.findByRole("menuitem", { name: "Density" });
    expect(density).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("menuitem", { name: "Theme: saving Dark" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await userEvent.click(density);
    expect(screen.queryByRole("menu", { name: "Density" })).toBeNull();

    answer(json({ theme: "dark", density: "comfortable" }));
    await waitFor(() => expect(html.getAttribute("data-sc-theme")).toBe("dark"));
    expect(html.hasAttribute("data-sc-density")).toBe(false);
    expect(bodies).toEqual([{ theme: "dark", density: "comfortable" }]);
    expect(await screen.findByRole("menuitem", { name: "Density" })).not.toHaveAttribute(
      "aria-disabled",
    );
  });

  it("shows an error toast and keeps the page as it was when the save fails", async () => {
    stubServer({
      "GET /api/v1/me": signedIn(),
      "PUT /api/v1/me/appearance": () => problem(500, "internal", "Internal Server Error"),
    });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    await choose("Theme", "Dark");
    const toast = await screen.findByRole("alertdialog");
    expect(within(toast).getByText("Your choice was not saved")).toBeInTheDocument();
    expect(html.hasAttribute("data-sc-theme")).toBe(false);
  });
});

describe("OrgSwitcher (B19, T9)", () => {
  it("is not in the DOM with one organization", async () => {
    stubServer({ "GET /api/v1/me": signedIn() });
    renderApp("/");
    await screen.findByRole("heading", { level: 1, name: "Home" });
    expect(document.querySelector('[data-slot="org-switcher"]')).toBeNull();
    expect(screen.queryByRole("button", { name: /Example Training/ })).toBeNull();
  });
});
