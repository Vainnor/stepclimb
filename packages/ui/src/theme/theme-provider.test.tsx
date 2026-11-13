// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useAppearance } from "./theme-provider";

/** Stand-ins for the OS settings: `matchMedia` for dark and more contrast, with `change` events. */
function emulateSystem(settings: { dark: boolean; contrast?: boolean }) {
  /** One entry per subscription, so two queries sharing a callback count twice. */
  const listeners: (() => void)[] = [];
  const state = { dark: settings.dark, contrast: settings.contrast ?? false };
  const query = (key: "dark" | "contrast") => ({
    get matches() {
      return state[key];
    },
    addEventListener: (_: string, fn: () => void) => listeners.push(fn),
    removeEventListener: (_: string, fn: () => void) => {
      const i = listeners.indexOf(fn);
      if (i >= 0) listeners.splice(i, 1);
    },
  });
  vi.stubGlobal(
    "matchMedia",
    vi.fn((q: string) => {
      if (q === "(prefers-color-scheme: dark)") return query("dark");
      if (q === "(prefers-contrast: more)") return query("contrast");
      throw new Error(`unexpected media query ${q}`);
    }),
  );
  return {
    listeners,
    set(next: Partial<typeof state>) {
      Object.assign(state, next);
      for (const fn of [...listeners]) fn();
    },
  };
}

function ShowAppearance() {
  const { theme, density, brand } = useAppearance();
  return <p>{`${theme} ${density} ${brand ?? "default"}`}</p>;
}

/** The outermost theme-provider element, or the innermost with `inner`. */
function element(inner = false): HTMLElement {
  const all = document.querySelectorAll<HTMLElement>('[data-slot="theme-provider"]');
  const root = inner ? all[all.length - 1] : all[0];
  if (!root) throw new Error("no theme-provider element");
  return root;
}

describe("ThemeProvider", () => {
  let system: ReturnType<typeof emulateSystem>;
  beforeEach(() => {
    system = emulateSystem({ dark: false });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("puts theme, density and brand on one element", () => {
    render(
      <ThemeProvider theme="high-contrast" density="compact" brand="vzdc">
        <ShowAppearance />
      </ThemeProvider>,
    );
    expect(element()).toHaveAttribute("data-sc-theme", "high-contrast");
    expect(element()).toHaveAttribute("data-sc-density", "compact");
    expect(element()).toHaveAttribute("data-sc-brand", "vzdc");
    expect(screen.getByText("high-contrast compact vzdc")).toBeInTheDocument();
  });

  it("is comfortable and the default brand unless told otherwise", () => {
    render(
      <ThemeProvider theme="light">
        <ShowAppearance />
      </ThemeProvider>,
    );
    expect(element()).toHaveAttribute("data-sc-density", "comfortable");
    expect(element()).not.toHaveAttribute("data-sc-brand");
    expect(screen.getByText("light comfortable default")).toBeInTheDocument();
  });

  it("follows the OS setting when no theme is set, and switches with it", () => {
    system = emulateSystem({ dark: true });
    render(
      <ThemeProvider>
        <ShowAppearance />
      </ThemeProvider>,
    );
    expect(element()).toHaveAttribute("data-sc-theme", "dark");
    expect(screen.getByText("dark comfortable default")).toBeInTheDocument();
    act(() => system.set({ dark: false }));
    expect(element()).toHaveAttribute("data-sc-theme", "light");
    expect(screen.getByText("light comfortable default")).toBeInTheDocument();
  });

  it("keeps a set theme when the OS setting changes", () => {
    render(
      <ThemeProvider theme="light">
        <ShowAppearance />
      </ThemeProvider>,
    );
    act(() => system.set({ dark: true }));
    expect(element()).toHaveAttribute("data-sc-theme", "light");
  });

  it("stops listening to the OS setting when it unmounts", () => {
    const { unmount } = render(<ThemeProvider>x</ThemeProvider>);
    expect(system.listeners).toHaveLength(2);
    unmount();
    expect(system.listeners).toHaveLength(0);
  });

  it("gives the nearest provider's appearance to a nested subtree", () => {
    render(
      <ThemeProvider theme="light">
        <ThemeProvider theme="dark" density="compact">
          <ShowAppearance />
        </ThemeProvider>
      </ThemeProvider>,
    );
    expect(screen.getByText("dark compact default")).toBeInTheDocument();
  });

  it("follows more contrast to high contrast before the color scheme", () => {
    system = emulateSystem({ dark: true, contrast: true });
    render(
      <ThemeProvider>
        <ShowAppearance />
      </ThemeProvider>,
    );
    expect(element()).toHaveAttribute("data-sc-theme", "high-contrast");
    act(() => system.set({ contrast: false }));
    expect(element()).toHaveAttribute("data-sc-theme", "dark");
    act(() => system.set({ contrast: true }));
    expect(element()).toHaveAttribute("data-sc-theme", "high-contrast");
  });

  it("gives a nested provider its parent's theme, density and brand when it sets none", () => {
    system = emulateSystem({ dark: true });
    render(
      <ThemeProvider theme="high-contrast" density="compact" brand="vzdc">
        <ThemeProvider>
          <ShowAppearance />
        </ThemeProvider>
      </ThemeProvider>,
    );
    expect(element(true)).toHaveAttribute("data-sc-theme", "high-contrast");
    expect(element(true)).toHaveAttribute("data-sc-density", "compact");
    expect(element(true)).toHaveAttribute("data-sc-brand", "vzdc");
    expect(screen.getByText("high-contrast compact vzdc")).toBeInTheDocument();
  });

  it("lets a nested provider override one value and keep the others", () => {
    render(
      <ThemeProvider theme="light" density="compact" brand="vzdc">
        <ThemeProvider theme="dark">
          <ShowAppearance />
        </ThemeProvider>
      </ThemeProvider>,
    );
    expect(element(true)).toHaveAttribute("data-sc-theme", "dark");
    expect(element(true)).toHaveAttribute("data-sc-density", "compact");
    expect(element(true)).toHaveAttribute("data-sc-brand", "vzdc");
  });

  it("passes className through for layout", () => {
    render(<ThemeProvider className="min-h-screen">x</ThemeProvider>);
    expect(element()).toHaveClass("min-h-screen");
  });

  it("refuses useAppearance() outside a provider", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<ShowAppearance />)).toThrow(/needs a <ThemeProvider>/);
  });
});
