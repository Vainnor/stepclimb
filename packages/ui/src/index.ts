// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb UI: the Button, the theme and strings providers so far; the rest of the M0 inventory is
// specified in docs/specs/m0/13-base-ui-components.md.
export { Button, type ButtonProps, button } from "./primitives/button";
export { type Catalog, type Messages, uiMessages } from "./strings/catalog";
export {
  createStrings,
  type MessageValues,
  type Strings,
  type StringsOptions,
  StringsProvider,
  type StringsProviderProps,
  useStepclimbStrings,
} from "./strings/strings";
export { defaultTerms, type Term, type TermForms, type TermOverrides } from "./strings/terms";
export {
  type Appearance,
  type Density,
  densities,
  type Theme,
  ThemeProvider,
  type ThemeProviderProps,
  themes,
  useAppearance,
} from "./theme/theme-provider";
