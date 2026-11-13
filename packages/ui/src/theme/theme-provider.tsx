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

/** The appearance a subtree is drawn in: the theme actually applied, the density and the brand. */
export interface Appearance {
  theme: Theme;
  density: Density;
  /** The `data-sc-brand` value; undefined for the default brand. */
  brand: string | undefined;
}

export interface ThemeProviderProps {
  /**
   * The theme. Without one, a nested provider keeps its parent's theme; an outermost provider
   * follows the OS: `prefers-contrast: more` selects high contrast (spec 12 Q5), otherwise
   * `prefers-color-scheme` selects light or dark, and it switches when either changes (plan, UI
   * tab -> What we borrow: `data-theme`, then `prefers-color-scheme`).
   */
  theme?: Theme;
  /** Compact sets the shorter control heights for dense views. Without one, the parent's density,
   * or comfortable at the outermost provider. */
  density?: Density;
  /**
   * The brand whose palette `data-sc-brand` selects, as named in the token CSS (`vzdc`). Without
   * one, the parent's brand, or the default brand at the outermost provider. The running app
   * takes its brand from the server's `/theme.css`; Storybook sets this to show the example
   * brands.
   */
  brand?: string;
  /** Layout only. */
  className?: string;
  children: ReactNode;
}

const contrastQuery = "(prefers-contrast: more)";
const darkQuery = "(prefers-color-scheme: dark)";

function subscribeToSystem(onChange: () => void): () => void {
  const queries = [window.matchMedia(contrastQuery), window.matchMedia(darkQuery)];
  for (const query of queries) query.addEventListener("change", onChange);
  return () => {
    for (const query of queries) query.removeEventListener("change", onChange);
  };
}

function systemTheme(): Theme {
  if (window.matchMedia(contrastQuery).matches) return "high-contrast";
  return window.matchMedia(darkQuery).matches ? "dark" : "light";
}

/** The SPA has no server render; this is the snapshot React asks for during hydration only. */
function serverTheme(): Theme {
  return "light";
}

const AppearanceContext = createContext<Appearance | null>(null);

/**
 * Puts the theme, density and brand on one element, which the token CSS needs: a brand's dark and
 * high-contrast colors match only when `data-sc-brand` and `data-sc-theme` sit together. A nested
 * provider repeats its parent's values for whatever it does not set.
 */
export function ThemeProvider({ theme, density, brand, className, children }: ThemeProviderProps) {
  const parent = use(AppearanceContext);
  const system = useSyncExternalStore(subscribeToSystem, systemTheme, serverTheme);
  const appearance: Appearance = {
    theme: theme ?? parent?.theme ?? system,
    density: density ?? parent?.density ?? "comfortable",
    brand: brand ?? parent?.brand,
  };
  return (
    <AppearanceContext value={appearance}>
      <div
        data-slot="theme-provider"
        data-sc-theme={appearance.theme}
        data-sc-density={appearance.density}
        data-sc-brand={appearance.brand}
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
