// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useAppearance } from "./theme-provider";

/** A stand-in for the OS dark setting: `matchMedia` with a `change` event the test can fire. */
function emulateSystem(dark: boolean) {
  const listeners = new Set<() => void>();
  const query = {
    matches: dark,
    addEventListener: (_: string, fn: () => void) => listeners.add(fn),
    removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
  };
  vi.stubGlobal(
    "matchMedia",
    vi.fn((q: string) => {
      if (q !== "(prefers-color-scheme: dark)") throw new Error(`unexpected media query ${q}`);
      return query;
    }),
  );
  return {
    listeners,
    set(next: boolean) {
      query.matches = next;
      for (const fn of listeners) fn();
    },
  };
}

function ShowAppearance() {
  const { theme, density } = useAppearance();
  return <p>{`${theme} ${density}`}</p>;
}

function element(): HTMLElement {
  const root = document.querySelector<HTMLElement>('[data-slot="theme-provider"]');
  if (!root) throw new Error("no theme-provider element");
  return root;
}

describe("ThemeProvider", () => {
  let system: ReturnType<typeof emulateSystem>;
  beforeEach(() => {
    system = emulateSystem(false);
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
    expect(screen.getByText("high-contrast compact")).toBeInTheDocument();
  });

  it("is comfortable and the default brand unless told otherwise", () => {
    render(
      <ThemeProvider theme="light">
        <ShowAppearance />
      </ThemeProvider>,
    );
    expect(element()).toHaveAttribute("data-sc-density", "comfortable");
    expect(element()).not.toHaveAttribute("data-sc-brand");
    expect(screen.getByText("light comfortable")).toBeInTheDocument();
  });

  it("follows the OS setting when no theme is set, and switches with it", () => {
    system = emulateSystem(true);
    render(
      <ThemeProvider>
        <ShowAppearance />
      </ThemeProvider>,
    );
    expect(element()).toHaveAttribute("data-sc-theme", "dark");
    expect(screen.getByText("dark comfortable")).toBeInTheDocument();
    act(() => system.set(false));
    expect(element()).toHaveAttribute("data-sc-theme", "light");
    expect(screen.getByText("light comfortable")).toBeInTheDocument();
  });

  it("keeps a set theme when the OS setting changes", () => {
    render(
      <ThemeProvider theme="light">
        <ShowAppearance />
      </ThemeProvider>,
    );
    act(() => system.set(true));
    expect(element()).toHaveAttribute("data-sc-theme", "light");
  });

  it("stops listening to the OS setting when it unmounts", () => {
    const { unmount } = render(<ThemeProvider>x</ThemeProvider>);
    expect(system.listeners.size).toBe(1);
    unmount();
    expect(system.listeners.size).toBe(0);
  });

  it("gives the nearest provider's appearance to a nested subtree", () => {
    render(
      <ThemeProvider theme="light">
        <ThemeProvider theme="dark" density="compact">
          <ShowAppearance />
        </ThemeProvider>
      </ThemeProvider>,
    );
    expect(screen.getByText("dark compact")).toBeInTheDocument();
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
