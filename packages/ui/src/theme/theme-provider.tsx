// SPDX-License-Identifier: AGPL-3.0-or-later
// ThemeProvider: puts the theme, density and brand attributes the token CSS reads on one element
// (packages/tokens/README.md: a brand's dark and high-contrast themes match only when
// `data-sc-brand` and `data-sc-theme` sit on the same element) and tells its children which
// appearance they are drawn in.
import { createContext, type ReactNode, use, useSyncExternalStore } from "react";

export const themes = ["light", "dark", "high-contrast"] as const;
export type Theme = (typeof themes)[number];

export const densities = ["comfortable", "compact"] as const;
export type Density = (typeof densities)[number];

/** The appearance a subtree is drawn in: the theme actually applied, and the density. */
export interface Appearance {
  theme: Theme;
  density: Density;
}

export interface ThemeProviderProps {
  /**
   * The theme. Without one, the provider follows the OS setting (`prefers-color-scheme`) and
   * switches when it changes, so a dark OS gets the dark theme (plan, UI tab -> What we borrow:
   * `data-theme`, then `prefers-color-scheme`).
   */
  theme?: Theme;
  /** Compact sets the shorter control heights for dense views. Comfortable by default. */
  density?: Density;
  /**
   * The brand whose palette `data-sc-brand` selects, as named in the token CSS (`vzdc`). Omitted,
   * the default brand applies. The running app takes its brand from the server's `/theme.css`;
   * Storybook sets this to show the example brands.
   */
  brand?: string;
  /** Layout only. */
  className?: string;
  children: ReactNode;
}

const darkQuery = "(prefers-color-scheme: dark)";

function subscribeToSystem(onChange: () => void): () => void {
  const query = window.matchMedia(darkQuery);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

function systemTheme(): Theme {
  return window.matchMedia(darkQuery).matches ? "dark" : "light";
}

/** The SPA has no server render; this is the snapshot React asks for during hydration only. */
function serverTheme(): Theme {
  return "light";
}

const AppearanceContext = createContext<Appearance | null>(null);

export function ThemeProvider({
  theme,
  density = "comfortable",
  brand,
  className,
  children,
}: ThemeProviderProps) {
  const system = useSyncExternalStore(subscribeToSystem, systemTheme, serverTheme);
  const applied = theme ?? system;
  return (
    <AppearanceContext value={{ theme: applied, density }}>
      <div
        data-slot="theme-provider"
        data-sc-theme={applied}
        data-sc-density={density}
        data-sc-brand={brand}
        className={className}
      >
        {children}
      </div>
    </AppearanceContext>
  );
}

/** The appearance of the nearest ThemeProvider. */
export function useAppearance(): Appearance {
  const appearance = use(AppearanceContext);
  if (!appearance) throw new Error("useAppearance() needs a <ThemeProvider> above it");
  return appearance;
}
