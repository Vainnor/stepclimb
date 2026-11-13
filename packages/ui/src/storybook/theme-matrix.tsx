// SPDX-License-Identifier: AGPL-3.0-or-later
// Story support, not part of the library: the brands Storybook shows and the matrix that draws a
// component in every brand, theme and density side by side (docs/specs/m0/13-base-ui-components.md
// B22). The two extreme brands join `brands` when the theme generator builds them (spec 12).
import type { ReactNode } from "react";
import {
  type Density,
  densities,
  type Theme,
  ThemeProvider,
  themes,
} from "../theme/theme-provider";

/** The brands the token CSS defines, by their `data-sc-brand` value; `default` sets none. */
export const brands = [
  { value: "default", title: "Stepclimb default" },
  { value: "vzdc", title: "vZDC example" },
] as const;
export type Brand = (typeof brands)[number]["value"];

export const themeTitles: Record<Theme, string> = {
  light: "Light",
  dark: "Dark",
  "high-contrast": "High contrast",
};

export const densityTitles: Record<Density, string> = {
  comfortable: "Comfortable",
  compact: "Compact",
};

/** `data-sc-brand` for a brand: none for the default brand. */
export function brandAttribute(brand: Brand): string | undefined {
  return brand === "default" ? undefined : brand;
}

export interface ThemeMatrixProps {
  /** Drawn once per cell, inside that cell's ThemeProvider. */
  children: ReactNode;
}

/**
 * One row per brand and density, one column per theme. A story that renders it sets
 * `parameters.themeWrapper: false` and the `theme-matrix` tag, so the story suite screenshots it
 * once per width instead of once per theme (apps/ui-docs/visual).
 */
export function ThemeMatrix({ children }: ThemeMatrixProps) {
  return (
    <div className="flex flex-col">
      {brands.flatMap((brand) =>
        densities.map((density) => (
          <div key={`${brand.value}-${density}`} className="grid grid-cols-1 lg:grid-cols-3">
            {themes.map((theme) => {
              const label = `${brand.title}, ${themeTitles[theme]}, ${densityTitles[density]}`;
              return (
                <ThemeProvider
                  key={theme}
                  theme={theme}
                  density={density}
                  brand={brandAttribute(brand.value)}
                >
                  <section
                    aria-label={label}
                    className="flex h-full flex-col gap-4 bg-canvas p-6 font-ui text-fg-default"
                  >
                    <h2 className="text-sm font-medium text-fg-sub">{label}</h2>
                    {children}
                  </section>
                </ThemeProvider>
              );
            })}
          </div>
        )),
      )}
    </div>
  );
}
