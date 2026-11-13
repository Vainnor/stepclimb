// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb UI: the Foundations, the Button, and the theme and strings providers so far; the rest of
// the M0 inventory is specified in docs/specs/m0/13-base-ui-components.md.
export { Button, type ButtonProps, button } from "./primitives/button";
export {
  Heading,
  type HeadingLevel,
  type HeadingProps,
  type HeadingSize,
  heading,
  headingSizes,
} from "./primitives/heading";
export { Kbd, type KbdProps, kbd } from "./primitives/kbd";
export { isExternalHref, Link, type LinkProps, link } from "./primitives/link";
export { Prose, type ProseProps, prose } from "./primitives/prose";
export { Separator, type SeparatorProps, separator } from "./primitives/separator";
export { Text, type TextProps, text } from "./primitives/text";
export { VisuallyHidden, type VisuallyHiddenProps } from "./primitives/visually-hidden";
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
export { Icon, type IconProps, type IconState, icon, iconWeights } from "./theme/icon";
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
